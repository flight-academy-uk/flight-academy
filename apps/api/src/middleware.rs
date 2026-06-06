//! HTTP middleware:
//!
//! * `x-request-id` propagation per ADR-004 §B.
//! * `dev_auth` — walking-skeleton subject extractor; replaced by real
//!   passwordless auth (ADR-001 §F, ADR-013) when that lands.
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

use std::collections::BTreeSet;

use axum::{
    extract::Request,
    http::{HeaderValue, header::HeaderName},
    middleware::Next,
    response::Response,
};
use flight_academy_auth::{ActorClass, Role, Subject, SubjectAttributes};
use flight_academy_core::Error;
use tower_http::request_id::{MakeRequestId, RequestId};
use uuid::Uuid;

use crate::error::ApiError;

pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Generates a UUID v7 per request. tower-http's bundled `MakeRequestUuid`
/// uses v4; we want v7 for time-ordering in audit / log correlation.
#[derive(Clone, Copy, Default)]
pub struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _request: &axum::http::Request<B>) -> Option<RequestId> {
        // UUID v7 hyphenated form is always a valid HeaderValue (ASCII, no
        // control chars), so the conversion cannot fail.
        let id = Uuid::now_v7().to_string();
        HeaderValue::from_str(&id).ok().map(RequestId::new)
    }
}

/// Walking-skeleton identity middleware. Reads a `Subject` from the
/// `FA_DEV_USER_ID` and `FA_DEV_TENANT_ID` environment variables and
/// attaches it to request extensions. Gated by `FA_DEV_AUTH=1` so it
/// never activates by accident; with the env unset, all routes
/// downstream of this layer return 401.
///
/// Removed when the real passwordless / WebAuthn auth subsystem
/// (ADR-001 §F, ADR-013) lands. Documented as **dev-only** in CONTRIBUTING
/// once that subsystem provides the production path.
pub async fn dev_auth(mut req: Request, next: Next) -> Result<Response, ApiError> {
    if std::env::var("FA_DEV_AUTH").as_deref() != Ok("1") {
        return Err(Error::Unauthorized.into());
    }
    let user_id = std::env::var("FA_DEV_USER_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(&s).ok())
        .ok_or(ApiError::from(Error::Unauthorized))?;
    let tenant_id = std::env::var("FA_DEV_TENANT_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(&s).ok())
        .ok_or(ApiError::from(Error::Unauthorized))?;

    // Roles are comma-separated in `FA_DEV_ROLES` (e.g. `tenant-admin`).
    // Unknown role strings are silently dropped so a config ahead of the
    // binary's known set degrades safely. Empty env var → no roles
    // (subject is a plain member).
    let roles: BTreeSet<Role> = std::env::var("FA_DEV_ROLES")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| Role::from_str_known(s.trim()))
        .collect();

    let subject = Subject {
        user_id,
        actor_class: ActorClass::Member,
        tenant_id: Some(tenant_id),
        roles,
        attributes: SubjectAttributes,
        elevation: None,
    };
    req.extensions_mut().insert(subject);
    Ok(next.run(req).await)
}
