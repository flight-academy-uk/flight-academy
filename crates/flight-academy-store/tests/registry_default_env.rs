//! Process-isolated env-var check for `CipherRegistry::with_v0_1_defaults`.
//!
//! `std::env::remove_var` mutates process-global state. Run as its own
//! integration test binary so it cannot race with any other test in
//! the workspace that touches `FA_DEFAULT_AEAD` — each integration
//! test binary is a separate `cargo test` process, so the binary
//! boundary is the isolation boundary.

use flight_academy_store::aead::{CipherRegistry, algo_id};

#[test]
fn default_is_gcm_siv_when_env_unset() {
    // SAFETY: `std::env::remove_var` requires `unsafe` because it is
    // not thread-safe. The binary boundary above guarantees this is
    // the only test running in this process, so the un-synchronised
    // mutation is sound.
    unsafe {
        std::env::remove_var("FA_DEFAULT_AEAD");
    }
    let registry = CipherRegistry::with_v0_1_defaults().unwrap();
    assert_eq!(registry.default_algo_id(), algo_id::AES_256_GCM_SIV);
}
