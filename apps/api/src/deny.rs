//! Recording Denies per ADR-033 §C.
//!
//! A lookup that the caller's visibility hides, or a slug nobody has, ends in
//! `404`; a policy that refuses an action on a visible resource ends in `403`.
//! Both are Denies, and ADR-009 §B audits every Deny. The row goes to the
//! caller's own chain, never the target's: the target may not exist, and
//! naming it would need the lookup that was hidden. A not-found row is the
//! same whether or not the target exists, so both cases do the same work
//! before the response. The slug the caller sent is recorded as a keyed
//! hash ([`AuditKey`]), so no caller-typed text reaches the chain.

use std::sync::Arc;

use axum::extract::MatchedPath;
use axum::http::Method;
use flight_academy_auth::{Action, Denied, Policy, Resource, Subject, authorise};
use flight_academy_core::Error;
use flight_academy_db::{Chain, Db};
use hmac::{Hmac, KeyInit, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::ApiError;

/// Shortest accepted audit key, in bytes: the output size of SHA-256.
pub const MIN_AUDIT_KEY_LEN: usize = 32;

/// Secret for the keyed hashes written in Deny rows. A tenant slug is
/// chosen by people and can carry a person's name (a sole trader's school
/// named after its owner), and audit rows cannot be erased, so the slug a
/// caller sent is recorded as HMAC-SHA-256 under this key, never as text.
/// An operator holding the key can still match a row to a slug by
/// hashing the candidate.
#[derive(Clone)]
pub struct AuditKey {
    bytes: Arc<Zeroizing<Vec<u8>>>,
}

impl AuditKey {
    /// Use `bytes` as the key. `None` when shorter than
    /// [`MIN_AUDIT_KEY_LEN`].
    pub fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        let bytes = Zeroizing::new(bytes);
        (bytes.len() >= MIN_AUDIT_KEY_LEN).then(|| Self {
            bytes: Arc::new(bytes),
        })
    }

    /// A fresh random key, for routers built without configuration
    /// (tests, spec emission). Hashes made under it match nothing else.
    pub fn random() -> Self {
        let mut bytes = vec![0u8; MIN_AUDIT_KEY_LEN];
        OsRng.fill_bytes(&mut bytes);
        Self::from_bytes(bytes).expect("random key has the minimum length")
    }

    /// Hex HMAC-SHA-256 of `label:value`. The label separates kinds of
    /// value hashed under the same key.
    pub fn mac_hex(&self, label: &str, value: &str) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.bytes).expect("HMAC accepts any key length");
        mac.update(label.as_bytes());
        mac.update(b":");
        mac.update(value.as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

impl core::fmt::Debug for AuditKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("AuditKey(..)")
    }
}

/// Reason recorded for a lookup that returned no row. Fixed before the
/// lookup runs and never derived from its result.
const NOT_VISIBLE_OR_ABSENT: &str = "not_visible_or_absent";

/// The request a Deny is recorded against: who asked, for what, on which
/// route, and the key exactly as they sent it.
pub struct DenyContext<'a> {
    pub db: &'a Db,
    pub key: &'a AuditKey,
    pub subject: &'a Subject,
    pub method: &'a Method,
    pub route: &'a MatchedPath,
    pub action: Action,
    /// The tenant slug from the path, as sent.
    pub requested_tenant: &'a str,
}

impl DenyContext<'_> {
    /// Record a lookup that found nothing visible and return the `404`.
    pub async fn not_found(&self, resource: &'static str) -> ApiError {
        match self.record(NOT_VISIBLE_OR_ABSENT).await {
            Ok(()) => Error::NotFound { resource }.into(),
            Err(err) => err,
        }
    }

    /// Run `policy`; on a Deny, record it and return the `403`.
    pub async fn authorise(
        &self,
        policy: &impl Policy,
        resource: &Resource,
    ) -> Result<(), ApiError> {
        match authorise(policy, self.subject, self.action, resource) {
            Ok(()) => Ok(()),
            Err(Denied { reason }) => {
                self.record(reason).await?;
                Err(Error::Forbidden.into())
            }
        }
    }

    async fn record(&self, reason: &'static str) -> Result<(), ApiError> {
        let chain = match self.subject.tenant_id {
            Some(tenant_id) => Chain::Tenant(tenant_id),
            None => Chain::User(self.subject.user_id),
        };
        let requested = if is_well_formed_slug(self.requested_tenant) {
            serde_json::json!({
                "tenant_slug_hmac": self.key.mac_hex("tenant-slug", self.requested_tenant),
            })
        } else {
            serde_json::json!({ "malformed": true })
        };
        let payload = serde_json::json!({
            "action": "deny",
            "operation": self.action.as_str(),
            "route": format!("{} {}", self.method, self.route.as_str()),
            "requested": requested,
            "reason": reason,
        });
        self.db
            .write_audit_event(
                chain,
                self.subject.actor_class.as_str(),
                Some(self.subject.user_id),
                payload,
            )
            .await?;
        Ok(())
    }
}

/// Whether `s` has the shape the `tenants.slug` CHECK constraint allows
/// (`^[a-z][a-z0-9-]{1,62}$`). Anything else a caller typed into the path
/// is recorded only as malformed, so free text does not reach the audit
/// chain (ADR-004 §B, §D).
fn is_well_formed_slug(s: &str) -> bool {
    let bytes = s.as_bytes();
    (2..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::{AuditKey, MIN_AUDIT_KEY_LEN, is_well_formed_slug};

    #[test]
    fn audit_key_needs_the_minimum_length() {
        assert!(AuditKey::from_bytes(vec![1; MIN_AUDIT_KEY_LEN - 1]).is_none());
        assert!(AuditKey::from_bytes(vec![1; MIN_AUDIT_KEY_LEN]).is_some());
    }

    #[test]
    fn keyed_hashes_depend_on_key_label_and_value() {
        let key = AuditKey::from_bytes(vec![1; 32]).unwrap();
        let other = AuditKey::from_bytes(vec![2; 32]).unwrap();
        let mac = key.mac_hex("tenant-slug", "alpha-academy");
        assert_eq!(mac.len(), 64);
        assert!(mac.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(mac, key.mac_hex("tenant-slug", "alpha-academy"));
        assert_ne!(mac, key.mac_hex("tenant-slug", "bravo-flight"));
        assert_ne!(mac, key.mac_hex("user-slug", "alpha-academy"));
        assert_ne!(mac, other.mac_hex("tenant-slug", "alpha-academy"));
    }

    #[test]
    fn audit_key_debug_hides_the_bytes() {
        let key = AuditKey::from_bytes(vec![0xab; 32]).unwrap();
        assert_eq!(format!("{key:?}"), "AuditKey(..)");
    }

    #[test]
    fn slug_shape_matches_the_tenants_check_constraint() {
        for ok in [
            "ab",
            "alpha-academy",
            "a1-2",
            &format!("a{}", "b".repeat(62)),
        ] {
            assert!(is_well_formed_slug(ok), "{ok:?}");
        }
        for bad in [
            "",
            "a",
            "1alpha",
            "-alpha",
            "Alpha",
            "alpha academy",
            "alpha_academy",
            "jane.doe@example.com",
            &format!("a{}", "b".repeat(63)),
        ] {
            assert!(!is_well_formed_slug(bad), "{bad:?}");
        }
    }
}
