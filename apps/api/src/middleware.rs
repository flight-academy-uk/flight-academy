//! HTTP middleware. `x-request-id` propagation per ADR-004 §B.
//!
//! Each request gets a UUID v7 (time-ordered, sortable) if no `x-request-id`
//! header is supplied; the same id is echoed back on the response. Handlers
//! reach the id via the `tower_http::request_id::RequestId` request extension
//! (e.g. for populating the problem+json `request_id` field — see
//! `error::ProblemDetails`).
//!
//! Time-ordering note: UUID v7 leaks coarse timestamp, which is fine for an
//! ingress-generated correlation id. Clients supplying their own id may
//! choose any string-shaped value; we do not validate the format because
//! tower-http already constrains it to a valid `HeaderValue`.

use axum::http::{HeaderValue, Request, header::HeaderName};
use tower_http::request_id::{MakeRequestId, RequestId};
use uuid::Uuid;

pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Generates a UUID v7 per request. tower-http's bundled `MakeRequestUuid`
/// uses v4; we want v7 for time-ordering in audit / log correlation.
#[derive(Clone, Copy, Default)]
pub struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        // UUID v7 hyphenated form is always a valid HeaderValue (ASCII, no
        // control chars), so the conversion cannot fail.
        let id = Uuid::now_v7().to_string();
        HeaderValue::from_str(&id).ok().map(RequestId::new)
    }
}
