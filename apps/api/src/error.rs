//! HTTP-layer rendering for the canonical `Error` enum.
//!
//! Produces an `application/problem+json` envelope per ADR-006 §F (RFC 9457).
//! The enum itself lives in `flight-academy-core` (ADR-005 §C); this module
//! is the HTTP-layer rendering and will move to `flight-academy-http-core`
//! when `apps/admin` lands (ADR-005 §F).
//!
//! Body shape:
//!
//! ```json
//! {
//!   "type":       "https://flight-academy.app/problems/<slug>",
//!   "title":      "...",
//!   "status":     <u16>,
//!   "detail":     "...",   // optional
//!   "instance":   "...",   // optional, populated by handler when meaningful
//!   "request_id": "..."    // optional; populated by handler via the
//!                          //   RequestId extractor when an error path
//!                          //   actually exists. /healthz is infallible,
//!                          //   so no current consumer exercises it.
//! }
//! ```
//!
//! Type URIs are stable and documented; treat changes as a contract break.

use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use flight_academy_core::Error;
use serde::Serialize;
use utoipa::ToSchema;

const TYPE_BASE: &str = "https://flight-academy.app/problems/";

/// RFC 9457 problem+json envelope. Serde renames `type_uri` to `type` since
/// `type` is a reserved Rust keyword.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// HTTP-layer wrapper around `flight_academy_core::Error` that implements
/// `IntoResponse`. Handlers return `Result<T, ApiError>`; `?` on a
/// `flight_academy_core::Result` converts automatically via `From<Error>`.
///
/// Lower-layer errors (`flight_academy_db::Error`, raw `sqlx::Error`) are
/// flattened to `Error::Internal` so the client never sees driver-level
/// detail; the underlying error is logged at error level for the
/// operator's eyes only. Real per-domain mapping (NotFound for missing
/// rows, Validation for FK conflicts, etc.) lands with the handlers
/// that need it.
pub struct ApiError {
    pub err: Error,
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        Self { err }
    }
}

impl From<flight_academy_db::Error> for ApiError {
    fn from(err: flight_academy_db::Error) -> Self {
        tracing::error!(?err, "db error");
        Self {
            err: Error::Internal,
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(?err, "sqlx error");
        Self {
            err: Error::Internal,
        }
    }
}

impl ApiError {
    fn classify(&self) -> (StatusCode, &'static str, &'static str, Option<String>) {
        match &self.err {
            Error::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Internal Server Error",
                None,
            ),
            Error::NotFound { resource } => (
                StatusCode::NOT_FOUND,
                "not-found",
                "Not Found",
                Some(format!("Resource '{resource}' not found.")),
            ),
            Error::Validation { field, message } => (
                StatusCode::BAD_REQUEST,
                "validation",
                "Validation Failed",
                Some(format!("Field '{field}': {message}")),
            ),
            Error::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Unauthorized",
                None,
            ),
            Error::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "Forbidden", None),
        }
    }
}

const PROBLEM_JSON: &str = "application/problem+json";

fn problem(
    status: StatusCode,
    slug: &str,
    title: &str,
    detail: Option<String>,
    request_id: Option<String>,
) -> Response {
    let body = ProblemDetails {
        type_uri: format!("{TYPE_BASE}{slug}"),
        title: title.to_string(),
        status: status.as_u16(),
        detail,
        instance: None,
        request_id,
    };
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(PROBLEM_JSON),
    );
    response
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, slug, title, detail) = self.classify();
        problem(
            status,
            slug,
            title,
            detail,
            crate::middleware::current_request_id(),
        )
    }
}

/// Rewrite an error response that is not already problem+json (the 408 from
/// the request timeout, axum's plain-text extractor rejections such as 413,
/// 415 and 422) into problem+json carrying `request_id`. The original body is
/// dropped: extractor rejection text can echo fragments of the request.
/// `Content-Type`, `Content-Length`, `Content-Encoding` and `Content-Range`
/// describe the old body and are replaced or removed; other headers are kept
/// (for example `Allow`). This applies to every route, including the HTML and
/// `/static` routes, whose errors become problem+json until HTML error pages
/// exist.
pub(crate) fn ensure_problem_json(response: Response, request_id: &str) -> Response {
    let status = response.status();
    let is_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v.as_bytes().starts_with(PROBLEM_JSON.as_bytes()));
    if !(status.is_client_error() || status.is_server_error()) || is_problem {
        return response;
    }
    let slug = match status {
        StatusCode::BAD_REQUEST => "bad-request",
        StatusCode::NOT_FOUND => "not-found",
        StatusCode::METHOD_NOT_ALLOWED => "method-not-allowed",
        StatusCode::REQUEST_TIMEOUT => "request-timeout",
        StatusCode::PAYLOAD_TOO_LARGE => "payload-too-large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported-media-type",
        StatusCode::UNPROCESSABLE_ENTITY => "unprocessable-entity",
        StatusCode::TOO_MANY_REQUESTS => "too-many-requests",
        s if s.is_server_error() => "internal",
        _ => "client-error",
    };
    let title = status.canonical_reason().unwrap_or("Error");
    let (mut parts, _) = response.into_parts();
    let rebuilt = problem(status, slug, title, None, Some(request_id.to_owned()));
    let (new_parts, body) = rebuilt.into_parts();
    for stale in [
        header::CONTENT_LENGTH,
        header::CONTENT_ENCODING,
        header::CONTENT_RANGE,
    ] {
        parts.headers.remove(stale);
    }
    for (name, value) in &new_parts.headers {
        parts.headers.insert(name, value.clone());
    }
    Response::from_parts(parts, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewritten_error_drops_stale_content_headers_and_keeps_others() {
        let inner = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::CONTENT_ENCODING, "br")
            .header(header::CONTENT_RANGE, "bytes 0-3/4")
            .header(header::CONTENT_LENGTH, "4")
            .header(header::ALLOW, "GET")
            .body(axum::body::Body::from("nope"))
            .unwrap();

        let out = ensure_problem_json(inner, "req-1");
        let h = out.headers();
        assert_eq!(out.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(h.get(header::CONTENT_TYPE).unwrap(), PROBLEM_JSON);
        assert!(h.get(header::CONTENT_ENCODING).is_none());
        assert!(h.get(header::CONTENT_RANGE).is_none());
        assert_eq!(h.get(header::ALLOW).unwrap(), "GET");
    }

    #[test]
    fn problem_json_and_success_responses_pass_through() {
        let ok = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_ENCODING, "br")
            .body(axum::body::Body::empty())
            .unwrap();
        let out = ensure_problem_json(ok, "req-1");
        assert_eq!(out.headers().get(header::CONTENT_ENCODING).unwrap(), "br");
    }
}
