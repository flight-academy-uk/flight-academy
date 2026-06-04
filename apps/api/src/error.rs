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
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        Self(err)
    }
}

impl ApiError {
    fn classify(&self) -> (StatusCode, &'static str, &'static str, Option<String>) {
        match &self.0 {
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

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, slug, title, detail) = self.classify();
        let body = ProblemDetails {
            type_uri: format!("{TYPE_BASE}{slug}"),
            title: title.to_string(),
            status: status.as_u16(),
            detail,
            instance: None,
            request_id: None,
        };
        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
