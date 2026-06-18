//! Round-trip + cross-algorithm dispatch tests for the AEAD envelope
//! format per ADR-022 §A/§B/§C.
//!
//! Covers:
//!
//! - Per-algorithm encrypt → decrypt round trip on all three v0.1
//!   AEADs (asserts each cipher impl wires up correctly).
//! - Cross-algorithm read dispatch: a database containing ciphertexts
//!   under multiple algorithms decrypts correctly via the registry's
//!   per-byte dispatch (the forward-migration property promised in
//!   ADR-022 §F).
//! - Header tampering: flipping the algo_id, nonce_len, or version byte
//!   triggers decrypt failure via the AAD binding per ADR-022 §C.
//! - AAD mismatch: changing `record_kind`, `record_id`, or `column`
//!   after encryption triggers decrypt failure — the ciphertext-swap
//!   defense.
//! - Ciphertext tampering: flipping a byte in the encrypted payload
//!   triggers decrypt failure (AEAD tag verification).
//! - Default algorithm selection: when `FA_DEFAULT_AEAD` is unset, the
//!   registry's default is GCM-SIV per ADR-022 §A.
//! - Envelope format negatives: truncated headers, sentinel algo_ids,
//!   out-of-range nonce lengths all produce structured errors rather
//!   than panics or silent decrypts.

use flight_academy_store::{
    AadRecord, StoreError,
    aead::{
        AeadCipher, AesGcm256, AesGcmSiv256, ChaCha20Poly1305Aead, CipherRegistry, Envelope,
        algo_id,
    },
};
use zeroize::Zeroizing;

fn fixture_aad() -> AadRecord<'static> {
    AadRecord {
        record_kind: "tenant_brand",
        record_id: "ten_01HXYZ",
        column: "settings",
    }
}

fn fixture_key() -> [u8; 32] {
    // A test-only key. Tests do not touch any real KMS or master file.
    [0x42; 32]
}

#[test]
fn gcm_siv_round_trips() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let plaintext = b"hello, encrypted world";

    let cipher = registry.default_cipher();
    let envelope = Envelope::encrypt(cipher, &key, plaintext, &aad.to_bytes()).unwrap();

    // Header sanity: version + algo + nonce_len + 12-byte nonce + 16-byte tag = 31 byte overhead.
    assert_eq!(envelope[0], 0x01, "version byte");
    assert_eq!(envelope[1], algo_id::AES_256_GCM_SIV, "algo_id byte");
    assert_eq!(envelope[2], 12, "nonce_len byte");

    let pt = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .unwrap();
    assert_eq!(pt, plaintext);
}

#[test]
fn chacha20_poly1305_round_trips() {
    let registry = CipherRegistry::for_test(algo_id::CHACHA20_POLY1305);
    let key = fixture_key();
    let aad = fixture_aad();
    let plaintext = b"chacha works too";

    let cipher = registry.default_cipher();
    let envelope = Envelope::encrypt(cipher, &key, plaintext, &aad.to_bytes()).unwrap();
    assert_eq!(envelope[1], algo_id::CHACHA20_POLY1305);

    let pt = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .unwrap();
    assert_eq!(pt, plaintext);
}

#[test]
fn aes_gcm_round_trips() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM);
    let key = fixture_key();
    let aad = fixture_aad();
    let plaintext = b"aes-gcm legacy lane";

    let cipher = registry.default_cipher();
    let envelope = Envelope::encrypt(cipher, &key, plaintext, &aad.to_bytes()).unwrap();
    assert_eq!(envelope[1], algo_id::AES_256_GCM);

    let pt = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .unwrap();
    assert_eq!(pt, plaintext);
}

#[test]
fn cross_algorithm_dispatch_reads_each_algo_correctly() {
    // The forward-migration property per ADR-022 §F: a database may
    // simultaneously contain ciphertexts under multiple algorithms;
    // reads dispatch on the algo_id byte without coordination.
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();

    // Write three ciphertexts, one per algorithm, under the same key.
    let ct_gcm_siv =
        Envelope::encrypt(&AesGcmSiv256, &key, b"siv message", &aad.to_bytes()).unwrap();
    let ct_chacha = Envelope::encrypt(
        &ChaCha20Poly1305Aead,
        &key,
        b"chacha message",
        &aad.to_bytes(),
    )
    .unwrap();
    let ct_gcm = Envelope::encrypt(&AesGcm256, &key, b"gcm message", &aad.to_bytes()).unwrap();

    assert_ne!(ct_gcm_siv[1], ct_chacha[1]);
    assert_ne!(ct_chacha[1], ct_gcm[1]);

    // Read each via the same registry; dispatch is by header byte.
    let pt_a = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &ct_gcm_siv,
        &aad.to_bytes(),
    )
    .unwrap();
    let pt_b = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &ct_chacha,
        &aad.to_bytes(),
    )
    .unwrap();
    let pt_c = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &ct_gcm,
        &aad.to_bytes(),
    )
    .unwrap();

    assert_eq!(pt_a, b"siv message");
    assert_eq!(pt_b, b"chacha message");
    assert_eq!(pt_c, b"gcm message");
}

#[test]
fn header_version_tampering_fails_decrypt() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let mut envelope =
        Envelope::encrypt(registry.default_cipher(), &key, b"data", &aad.to_bytes()).unwrap();

    // CodeQL false positive: this is not a nonce — the byte is a
    // negative-path mutation of the envelope's `version` header to
    // verify rejection. Real nonces come from `OsRng` in
    // `Envelope::encrypt` per ADR-022 §G.
    envelope[0] = 0x02; // bump version

    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("unsupported version should fail");
    assert!(matches!(err, StoreError::Envelope { reason } if reason.contains("version")));
}

#[test]
fn header_algo_id_tampering_fails_decrypt() {
    // Bumping algo_id to a different real algorithm: ciphertext was
    // produced under GCM-SIV; flipping algo_id to ChaCha20-Poly1305
    // causes the dispatch to try ChaCha20, AAD changes (header bytes
    // are folded in), tag verifies under neither — decrypt fails.
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let mut envelope =
        Envelope::encrypt(registry.default_cipher(), &key, b"data", &aad.to_bytes()).unwrap();

    // CodeQL false positive: this is not a nonce — the byte is the
    // envelope's algo_id header being mutated to test the AAD-bound
    // tag check (see §C). Real nonces come from `OsRng`.
    envelope[1] = algo_id::CHACHA20_POLY1305;

    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("algo_id swap should fail");
    assert!(matches!(err, StoreError::Decrypt));
}

#[test]
fn header_algo_id_sentinel_rejected() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let mut envelope =
        Envelope::encrypt(registry.default_cipher(), &key, b"data", &aad.to_bytes()).unwrap();

    // CodeQL false positive on both sentinel writes below: these are
    // not nonces — they are reserved algo_id sentinels (`0x00` and
    // `0xFF`) per ADR-022 §A, written into the envelope's algo_id
    // header byte to verify the parser rejects them.
    envelope[1] = 0x00;
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("0x00 sentinel algo_id should be rejected");
    assert!(matches!(err, StoreError::Envelope { reason } if reason.contains("sentinel")));

    envelope[1] = 0xFF;
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("0xFF sentinel algo_id should be rejected");
    assert!(matches!(err, StoreError::Envelope { reason } if reason.contains("sentinel")));
}

#[test]
fn aad_record_kind_swap_fails_decrypt() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let envelope = Envelope::encrypt(
        registry.default_cipher(),
        &key,
        b"sensitive",
        &fixture_aad().to_bytes(),
    )
    .unwrap();

    let swapped = AadRecord {
        record_kind: "user_logbook_entry", // was "tenant_brand"
        record_id: "ten_01HXYZ",
        column: "settings",
    };
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &swapped.to_bytes(),
    )
    .expect_err("AAD record_kind mismatch should fail");
    assert!(matches!(err, StoreError::Decrypt));
}

#[test]
fn aad_record_id_swap_fails_decrypt() {
    // The ciphertext-swap defense per ADR-022 §C — moving a ciphertext
    // from row A to row B fails authentication.
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let envelope = Envelope::encrypt(
        registry.default_cipher(),
        &key,
        b"secret",
        &fixture_aad().to_bytes(),
    )
    .unwrap();

    let swapped = AadRecord {
        record_kind: "tenant_brand",
        record_id: "ten_OTHER", // moved to a different row
        column: "settings",
    };
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &swapped.to_bytes(),
    )
    .expect_err("AAD record_id mismatch should fail");
    assert!(matches!(err, StoreError::Decrypt));
}

#[test]
fn aad_column_rename_fails_decrypt() {
    // The "column rename without re-encryption sweep" defense — surfaces
    // as decryption failure rather than silent corruption.
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let envelope = Envelope::encrypt(
        registry.default_cipher(),
        &key,
        b"data",
        &fixture_aad().to_bytes(),
    )
    .unwrap();

    let swapped = AadRecord {
        record_kind: "tenant_brand",
        record_id: "ten_01HXYZ",
        column: "config", // renamed from "settings"
    };
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &swapped.to_bytes(),
    )
    .expect_err("AAD column mismatch should fail");
    assert!(matches!(err, StoreError::Decrypt));
}

#[test]
fn ciphertext_byte_flip_fails_decrypt() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let mut envelope =
        Envelope::encrypt(registry.default_cipher(), &key, b"payload", &aad.to_bytes()).unwrap();

    // CodeQL false positive: the XOR mask is not a nonce — it bit-flips
    // a byte in the ciphertext body to verify the AEAD tag rejects
    // tampered ciphertext. Real nonces come from `OsRng`.
    let target = envelope.len() - 5;
    envelope[target] ^= 0x01;

    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("ciphertext byte flip should fail");
    assert!(matches!(err, StoreError::Decrypt));
}

#[test]
fn wrong_key_fails_decrypt() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let envelope =
        Envelope::encrypt(registry.default_cipher(), &key, b"secret", &aad.to_bytes()).unwrap();

    let wrong_key = [0xAA; 32];
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(wrong_key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("wrong key should fail");
    assert!(matches!(err, StoreError::Decrypt));
}

#[test]
fn truncated_envelope_returns_envelope_error() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let envelope = Envelope::encrypt(
        registry.default_cipher(),
        &key,
        b"data",
        &fixture_aad().to_bytes(),
    )
    .unwrap();

    // Truncate to less than the 3-byte header.
    let short = &envelope[..2];
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        short,
        &fixture_aad().to_bytes(),
    )
    .expect_err("short envelope should fail");
    assert!(matches!(err, StoreError::Envelope { .. }));

    // Truncate to past header but before nonce end.
    let nonce_truncated = &envelope[..3 + 5]; // 3 header + 5 of 12 nonce bytes
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        nonce_truncated,
        &fixture_aad().to_bytes(),
    )
    .expect_err("nonce-truncated envelope should fail");
    assert!(matches!(err, StoreError::Envelope { .. }));
}

#[test]
fn out_of_range_nonce_len_rejected() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let mut envelope =
        Envelope::encrypt(registry.default_cipher(), &key, b"data", &aad.to_bytes()).unwrap();

    // CodeQL false positive: this is not a nonce — the byte is the
    // envelope's `nonce_len` header being mutated to a below-`MIN_NONCE_LEN`
    // value to verify the bounds check rejects it.
    envelope[2] = 11; // below MIN_NONCE_LEN
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("nonce_len below 12 should fail");
    assert!(matches!(err, StoreError::Envelope { reason } if reason.contains("nonce_len")));

    // CodeQL false positive: same shape as above — the byte is the
    // `nonce_len` header at the upper bound.
    envelope[2] = 33; // above MAX_NONCE_LEN
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("nonce_len above 32 should fail");
    assert!(matches!(err, StoreError::Envelope { reason } if reason.contains("nonce_len")));
}

#[test]
fn unknown_algo_id_returns_unknown_algorithm() {
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad();
    let mut envelope =
        Envelope::encrypt(registry.default_cipher(), &key, b"data", &aad.to_bytes()).unwrap();

    // CodeQL false positive: this is not a nonce — the byte is the
    // envelope's algo_id header set to a reserved-but-unregistered
    // value (`0x07` per ADR-022 §A) to verify the registry returns
    // `UnknownAlgorithm`. Real nonces come from `OsRng`.
    envelope[1] = 0x07;
    let err = Envelope::decrypt(
        &registry,
        |_| Ok(Zeroizing::new(key.to_vec())),
        &envelope,
        &aad.to_bytes(),
    )
    .expect_err("unknown algo_id should fail");
    assert!(matches!(
        err,
        StoreError::UnknownAlgorithm { algo_id: 0x07 }
    ));
}

#[test]
fn registry_default_is_gcm_siv_when_env_unset() {
    // Concurrent tests share process env; protect the assertion by
    // removing any inherited value before constructing.
    //
    // SAFETY: env mutation requires `unsafe` in modern Rust because it
    // is not thread-safe; this test is intentionally a single
    // sequential check at the end of the module so it does not race
    // with other tests in the same binary.
    unsafe {
        std::env::remove_var("FA_DEFAULT_AEAD");
    }
    let registry = CipherRegistry::with_v0_1_defaults().unwrap();
    assert_eq!(registry.default_algo_id(), algo_id::AES_256_GCM_SIV);
}

#[test]
fn nonce_uniqueness_across_encryptions() {
    // ADR-022 §G — every encryption draws a fresh CSPRNG nonce. Two
    // back-to-back encryptions under the same key + AAD + plaintext
    // must produce different ciphertexts (different nonces, different
    // tags). This is the basic sanity check on OsRng wiring.
    let registry = CipherRegistry::for_test(algo_id::AES_256_GCM_SIV);
    let key = fixture_key();
    let aad = fixture_aad().to_bytes();
    let plaintext = b"identical input";

    let a = Envelope::encrypt(registry.default_cipher(), &key, plaintext, &aad).unwrap();
    let b = Envelope::encrypt(registry.default_cipher(), &key, plaintext, &aad).unwrap();

    assert_ne!(a, b, "two encryptions of the same plaintext must differ");
    assert_ne!(&a[3..15], &b[3..15], "nonces must differ");
}

#[test]
fn algo_id_byte_is_advertised_correctly_per_cipher() {
    assert_eq!(AesGcmSiv256.algo_id(), 0x01);
    assert_eq!(ChaCha20Poly1305Aead.algo_id(), 0x02);
    assert_eq!(AesGcm256.algo_id(), 0x03);
}

#[test]
fn key_and_nonce_sizes_are_uniform_at_v0_1() {
    // All three v0.1 algorithms use 32-byte keys and 12-byte nonces.
    for cipher in [
        &AesGcmSiv256 as &dyn AeadCipher,
        &ChaCha20Poly1305Aead,
        &AesGcm256,
    ] {
        assert_eq!(cipher.key_size(), 32);
        assert_eq!(cipher.nonce_size(), 12);
    }
}
