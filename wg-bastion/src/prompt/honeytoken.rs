//! [`HoneytokenStore`] — AES-256-GCM encrypted canary tokens with rotation
//! and egress detection.
//!
//! Generates a pool of high-entropy canary strings, encrypts them with
//! AES-256-GCM (random nonce per token), and uses Aho-Corasick multi-pattern
//! matching to detect token leakage in LLM output.

use crate::pipeline::content::Content;
use crate::pipeline::outcome::Severity;
use aho_corasick::AhoCorasick;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, Salt};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use zeroize::Zeroizing;

// ── Constants ──────────────────────────────────────────────────────────

const AES_KEY_LEN: usize = 32;
const HMAC_KEY_LEN: usize = 32;
const GCM_TAG_LEN: usize = 16;
const INJECT_COUNT: usize = 3;

const ENC_INFO: &[u8] = b"wg-bastion-honeytoken-aes-v1";
const HMAC_INFO: &[u8] = b"wg-bastion-honeytoken-hmac-v1";

// ── Hex helpers ────────────────────────────────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

// ── KeySource ──────────────────────────────────────────────────────────

/// Source of the master encryption key.
#[derive(Clone)]
#[non_exhaustive]
pub enum KeySource {
    /// Load key from the named environment variable (hex-encoded).
    EnvVar(String),
    /// Key provided directly as raw bytes (must be ≥ 32 bytes).
    Bytes(Zeroizing<Vec<u8>>),
}

impl fmt::Debug for KeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnvVar(var) => f.debug_tuple("EnvVar").field(var).finish(),
            Self::Bytes(_) => f.debug_tuple("Bytes").field(&"[REDACTED]").finish(),
        }
    }
}

/// Resolve a [`KeySource`] into raw key bytes.
fn resolve_key(source: &KeySource) -> Result<Zeroizing<Vec<u8>>, HoneytokenError> {
    match source {
        KeySource::EnvVar(var) => {
            let env_value = std::env::var(var).map_err(|_| HoneytokenError::KeyNotFound {
                env_var: var.clone(),
            })?;
            let bytes =
                hex_decode(&env_value).map_err(|()| HoneytokenError::InvalidKeyMaterial {
                    reason: format!("environment variable '{var}' does not contain valid hex"),
                })?;
            Ok(Zeroizing::new(bytes))
        }
        KeySource::Bytes(bytes) => Ok(bytes.clone()),
    }
}

// ── HoneytokenConfig ───────────────────────────────────────────────────

/// Configuration for the [`HoneytokenStore`].
#[derive(Debug, Clone)]
pub struct HoneytokenConfig {
    /// Entropy bits per generated token (default 128).
    pub token_entropy_bits: u32,
    /// Number of tokens in the pool (default 50).
    pub pool_size: usize,
    /// How often to rotate tokens (default 7 days).
    pub rotation_interval: Duration,
    /// Source of the master encryption key.
    pub key_source: KeySource,
}

impl HoneytokenConfig {
    /// Start building a [`HoneytokenConfig`].
    #[must_use]
    pub fn builder(key_source: KeySource) -> HoneytokenConfigBuilder {
        HoneytokenConfigBuilder {
            config: HoneytokenConfig {
                token_entropy_bits: 128,
                pool_size: 50,
                rotation_interval: Duration::from_secs(7 * 24 * 3600),
                key_source,
            },
        }
    }
}

// ── HoneytokenConfigBuilder ────────────────────────────────────────────

/// Builder for [`HoneytokenConfig`].
#[derive(Debug)]
pub struct HoneytokenConfigBuilder {
    config: HoneytokenConfig,
}

impl HoneytokenConfigBuilder {
    /// Set the entropy bits per generated token.
    #[must_use]
    pub fn token_entropy_bits(mut self, bits: u32) -> Self {
        self.config.token_entropy_bits = bits;
        self
    }

    /// Set the pool size.
    #[must_use]
    pub fn pool_size(mut self, size: usize) -> Self {
        self.config.pool_size = size;
        self
    }

    /// Set the rotation interval.
    #[must_use]
    pub fn rotation_interval(mut self, interval: Duration) -> Self {
        self.config.rotation_interval = interval;
        self
    }

    /// Build the config.
    #[must_use]
    pub fn build(self) -> HoneytokenConfig {
        self.config
    }
}

// ── Honeytoken ─────────────────────────────────────────────────────────

/// A single canary token with encrypted backing and HMAC fingerprint.
pub struct Honeytoken {
    /// Unique identifier for this token.
    pub id: String,
    /// The plaintext canary string (high-entropy hex).
    pub plaintext: Zeroizing<String>,
    /// Encrypted form: nonce (12) ‖ ciphertext ‖ GCM tag (16).
    pub encrypted: Vec<u8>,
    /// Creation timestamp.
    pub created_at: SystemTime,
    /// Hex-encoded HMAC-SHA256 fingerprint.
    pub hmac_fingerprint: String,
    /// `true` for rotated-out tokens still used for detection.
    pub detection_only: bool,
}

/// Custom Debug — never expose plaintext.
impl fmt::Debug for Honeytoken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Honeytoken")
            .field("id", &self.id)
            .field("plaintext", &"[REDACTED]")
            .field("encrypted", &format!("[{} bytes]", self.encrypted.len()))
            .field("created_at", &self.created_at)
            .field("hmac_fingerprint", &self.hmac_fingerprint)
            .field("detection_only", &self.detection_only)
            .finish()
    }
}

// ── HoneytokenDetection ────────────────────────────────────────────────

/// A detected honeytoken in LLM output.
#[derive(Debug, Clone)]
pub struct HoneytokenDetection {
    /// ID of the leaked token.
    pub token_id: String,
    /// HMAC fingerprint (never the plaintext — never log plaintext).
    pub hmac_fingerprint: String,
    /// Byte range where the token was found.
    pub position: Range<usize>,
    /// Always [`Severity::Critical`].
    pub severity: Severity,
}

// ── HoneytokenError ────────────────────────────────────────────────────

/// Errors from honeytoken operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HoneytokenError {
    /// The provided key is shorter than the required length.
    #[error("key too short: expected {expected} bytes, got {actual}")]
    KeyTooShort {
        /// Required key length.
        expected: usize,
        /// Actual key length.
        actual: usize,
    },

    /// The specified environment variable was not found.
    #[error("key not found in environment variable '{env_var}'")]
    KeyNotFound {
        /// Name of the missing variable.
        env_var: String,
    },

    /// AES-256-GCM encryption failed.
    #[error("encryption failed: {reason}")]
    EncryptionFailed {
        /// What went wrong.
        reason: String,
    },

    /// AES-256-GCM decryption failed.
    #[error("decryption failed: {reason}")]
    DecryptionFailed {
        /// What went wrong.
        reason: String,
    },

    /// Key material is invalid or could not be derived.
    #[error("invalid key material: {reason}")]
    InvalidKeyMaterial {
        /// What went wrong.
        reason: String,
    },
}

// ── HKDF helper ────────────────────────────────────────────────────────

struct HkdfKeyLen(usize);

impl ring::hkdf::KeyType for HkdfKeyLen {
    fn len(&self) -> usize {
        self.0
    }
}

fn derive_subkey(
    prk: &ring::hkdf::Prk,
    info: &[u8],
    len: usize,
) -> Result<Zeroizing<Vec<u8>>, HoneytokenError> {
    let info_refs = [info];
    let okm = prk.expand(&info_refs, HkdfKeyLen(len)).map_err(|_| {
        HoneytokenError::InvalidKeyMaterial {
            reason: "HKDF expand failed".into(),
        }
    })?;
    let mut out = Zeroizing::new(vec![0u8; len]);
    okm.fill(&mut out)
        .map_err(|_| HoneytokenError::InvalidKeyMaterial {
            reason: "HKDF fill failed".into(),
        })?;
    Ok(out)
}

// ── Standalone crypto helpers ──────────────────────────────────────────

fn encrypt_impl(
    key: &LessSafeKey,
    rng: &SystemRandom,
    plaintext: &[u8],
) -> Result<Vec<u8>, HoneytokenError> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| HoneytokenError::EncryptionFailed {
            reason: "failed to generate random nonce".into(),
        })?;

    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| HoneytokenError::EncryptionFailed {
            reason: "AES-256-GCM seal failed".into(),
        })?;

    // nonce (12) ‖ ciphertext ‖ tag (16)
    let mut result = Vec::with_capacity(NONCE_LEN + in_out.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&in_out);
    Ok(result)
}

fn decrypt_impl(key: &LessSafeKey, ciphertext: &[u8]) -> Result<Vec<u8>, HoneytokenError> {
    if ciphertext.len() < NONCE_LEN + GCM_TAG_LEN {
        return Err(HoneytokenError::DecryptionFailed {
            reason: format!(
                "ciphertext too short: {} bytes (minimum {})",
                ciphertext.len(),
                NONCE_LEN + GCM_TAG_LEN
            ),
        });
    }

    let (nonce_bytes, ct_and_tag) = ciphertext.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| {
        HoneytokenError::DecryptionFailed {
            reason: "invalid nonce length".into(),
        }
    })?;

    let mut buf = ct_and_tag.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::empty(), &mut buf)
        .map_err(|_| HoneytokenError::DecryptionFailed {
            reason: "AES-256-GCM open failed (bad key, nonce, or tampered data)".into(),
        })?;

    Ok(plaintext.to_vec())
}

fn generate_pool_impl(
    config: &HoneytokenConfig,
    enc_key: &LessSafeKey,
    hmac_key: &hmac::Key,
    rng: &SystemRandom,
) -> Result<Vec<Honeytoken>, HoneytokenError> {
    let byte_count = (config.token_entropy_bits / 8) as usize;
    let mut tokens = Vec::with_capacity(config.pool_size);

    for _ in 0..config.pool_size {
        let mut raw = Zeroizing::new(vec![0u8; byte_count]);
        rng.fill(&mut raw)
            .map_err(|_| HoneytokenError::EncryptionFailed {
                reason: "RNG fill failed during token generation".into(),
            })?;

        let plaintext = Zeroizing::new(hex_encode(&raw));
        let encrypted = encrypt_impl(enc_key, rng, plaintext.as_bytes())?;
        let hmac_tag = hmac::sign(hmac_key, plaintext.as_bytes());
        let hmac_fingerprint = hex_encode(hmac_tag.as_ref());

        let mut id_bytes = [0u8; 6];
        rng.fill(&mut id_bytes)
            .map_err(|_| HoneytokenError::EncryptionFailed {
                reason: "RNG fill failed during ID generation".into(),
            })?;

        tokens.push(Honeytoken {
            id: format!("ht-{}", hex_encode(&id_bytes)),
            plaintext,
            encrypted,
            created_at: SystemTime::now(),
            hmac_fingerprint,
            detection_only: false,
        });
    }

    Ok(tokens)
}

fn build_automaton(tokens: &[Honeytoken]) -> AhoCorasick {
    let patterns: Vec<&str> = tokens.iter().map(|t| t.plaintext.as_str()).collect();
    AhoCorasick::new(&patterns).expect("honeytoken patterns are valid literals")
}

/// Find the nearest char boundary at or before `pos`.
fn nearest_char_boundary(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    let mut p = pos;
    while !s.is_char_boundary(p) && p > 0 {
        p -= 1;
    }
    p
}

#[allow(clippy::cast_possible_truncation)] // modular index: truncation is harmless
fn random_indices(rng: &SystemRandom, max: usize, n: usize) -> Result<Vec<usize>, HoneytokenError> {
    let n = n.min(max);
    let mut selected = Vec::with_capacity(n);
    let mut attempts = 0;
    while selected.len() < n && attempts < n * 20 {
        let mut buf = [0u8; 8];
        rng.fill(&mut buf)
            .map_err(|_| HoneytokenError::EncryptionFailed {
                reason: "RNG failed during index selection".into(),
            })?;
        let idx = (u64::from_ne_bytes(buf) as usize) % max;
        if !selected.contains(&idx) {
            selected.push(idx);
        }
        attempts += 1;
    }
    Ok(selected)
}

// ── Store inner state ──────────────────────────────────────────────────

struct StoreState {
    tokens: Vec<Honeytoken>,
    automaton: AhoCorasick,
}

// ── HoneytokenStore ────────────────────────────────────────────────────

/// AES-256-GCM encrypted canary token store with Aho-Corasick detection.
///
/// Manages a pool of high-entropy honeytoken strings that can be injected
/// into prompts and later detected in LLM output to identify data leakage.
pub struct HoneytokenStore {
    state: Arc<RwLock<StoreState>>,
    enc_key: LessSafeKey,
    hmac_key: hmac::Key,
    rng: SystemRandom,
    config: HoneytokenConfig,
}

impl fmt::Debug for HoneytokenStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pool_size = self.state.read().map(|s| s.tokens.len()).unwrap_or(0);
        f.debug_struct("HoneytokenStore")
            .field("pool_size", &pool_size)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl HoneytokenStore {
    /// Create a new store, deriving subkeys via HKDF and generating the
    /// initial token pool.
    ///
    /// # Errors
    ///
    /// Returns [`HoneytokenError`] if the key is too short, missing, or
    /// cryptographic operations fail.
    pub fn new(config: HoneytokenConfig) -> Result<Self, HoneytokenError> {
        let master_key = resolve_key(&config.key_source)?;
        if master_key.len() < AES_KEY_LEN {
            return Err(HoneytokenError::KeyTooShort {
                expected: AES_KEY_LEN,
                actual: master_key.len(),
            });
        }

        // HKDF-SHA256: extract PRK, then expand into two independent subkeys
        let salt = Salt::new(HKDF_SHA256, &[]);
        let prk = salt.extract(&master_key);

        let enc_key_bytes = derive_subkey(&prk, ENC_INFO, AES_KEY_LEN)?;
        let hmac_key_bytes = derive_subkey(&prk, HMAC_INFO, HMAC_KEY_LEN)?;

        let unbound = UnboundKey::new(&AES_256_GCM, &enc_key_bytes).map_err(|_| {
            HoneytokenError::InvalidKeyMaterial {
                reason: "failed to create AES-256-GCM unbound key".into(),
            }
        })?;
        let enc_key = LessSafeKey::new(unbound);
        let hmac_key = hmac::Key::new(hmac::HMAC_SHA256, &hmac_key_bytes);
        let rng = SystemRandom::new();

        let pool = generate_pool_impl(&config, &enc_key, &hmac_key, &rng)?;
        let automaton = build_automaton(&pool);

        Ok(Self {
            state: Arc::new(RwLock::new(StoreState {
                tokens: pool,
                automaton,
            })),
            enc_key,
            hmac_key,
            rng,
            config,
        })
    }

    /// Generate a fresh pool of `pool_size` random high-entropy tokens.
    ///
    /// # Errors
    ///
    /// Returns [`HoneytokenError::EncryptionFailed`] if random generation
    /// or encryption fails.
    pub fn generate_pool(&self) -> Result<Vec<Honeytoken>, HoneytokenError> {
        generate_pool_impl(&self.config, &self.enc_key, &self.hmac_key, &self.rng)
    }

    /// Inject a random subset of tokens into a prompt as comment markers.
    ///
    /// Returns the modified prompt and the IDs of injected tokens.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    ///
    /// # Errors
    ///
    /// Returns [`HoneytokenError::EncryptionFailed`] if random selection fails.
    pub fn inject_into_prompt(
        &self,
        prompt: &str,
    ) -> Result<(String, Vec<String>), HoneytokenError> {
        let state = self.state.read().unwrap();
        let active: Vec<&Honeytoken> = state.tokens.iter().filter(|t| !t.detection_only).collect();

        if active.is_empty() {
            return Ok((prompt.to_owned(), Vec::new()));
        }

        let inject_count = INJECT_COUNT.min(active.len());
        let indices = random_indices(&self.rng, active.len(), inject_count)?;

        let mut injected_ids = Vec::with_capacity(inject_count);
        let mut markers = Vec::with_capacity(inject_count);
        for &idx in &indices {
            let token = active[idx];
            injected_ids.push(token.id.clone());
            markers.push(format!("<!-- wg-canary:{} -->", &*token.plaintext));
        }

        // Insert markers at evenly-spaced positions in the prompt
        let mut result = String::with_capacity(
            prompt.len() + markers.iter().map(|m| m.len() + 1).sum::<usize>(),
        );
        let segment_len = if inject_count > 0 {
            prompt.len() / (inject_count + 1)
        } else {
            prompt.len()
        };
        let mut last = 0;

        for (i, marker) in markers.iter().enumerate() {
            let pos = nearest_char_boundary(prompt, segment_len * (i + 1));
            result.push_str(&prompt[last..pos]);
            result.push_str(marker);
            last = pos;
        }
        result.push_str(&prompt[last..]);

        Ok((result, injected_ids))
    }

    /// Scan LLM output for leaked honeytokens using Aho-Corasick.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    #[must_use] pub fn detect_in_output(&self, output: &Content) -> Vec<HoneytokenDetection> {
        let text = output.as_text();
        let state = self.state.read().unwrap();

        let mut detections = Vec::new();
        for mat in state.automaton.find_iter(text.as_ref()) {
            let token = &state.tokens[mat.pattern().as_usize()];
            detections.push(HoneytokenDetection {
                token_id: token.id.clone(),
                hmac_fingerprint: token.hmac_fingerprint.clone(),
                position: mat.start()..mat.end(),
                severity: Severity::Critical,
            });
        }

        detections
    }

    /// Rotate: generate new tokens, mark old ones as detection-only,
    /// and rebuild the detection automaton.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    ///
    /// # Errors
    ///
    /// Returns [`HoneytokenError`] if pool generation fails.
    pub fn rotate(&self) -> Result<(), HoneytokenError> {
        let new_tokens = self.generate_pool()?;

        let mut state = self.state.write().unwrap();
        for token in &mut state.tokens {
            token.detection_only = true;
        }
        state.tokens.extend(new_tokens);
        state.automaton = build_automaton(&state.tokens);

        Ok(())
    }

    /// Encrypt plaintext with AES-256-GCM.
    ///
    /// Output format: nonce (12) ‖ ciphertext ‖ GCM tag (16).
    ///
    /// # Errors
    ///
    /// Returns [`HoneytokenError::EncryptionFailed`] on failure.
    pub fn encrypt_token(&self, plaintext: &[u8]) -> Result<Vec<u8>, HoneytokenError> {
        encrypt_impl(&self.enc_key, &self.rng, plaintext)
    }

    /// Decrypt AES-256-GCM ciphertext.
    ///
    /// Expects input format: nonce (12) ‖ ciphertext ‖ GCM tag (16).
    ///
    /// # Errors
    ///
    /// Returns [`HoneytokenError::DecryptionFailed`] on failure.
    pub fn decrypt_token(&self, ciphertext: &[u8]) -> Result<Vec<u8>, HoneytokenError> {
        decrypt_impl(&self.enc_key, ciphertext)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> Zeroizing<Vec<u8>> {
        Zeroizing::new(vec![0x42u8; 32])
    }

    fn test_config(pool_size: usize) -> HoneytokenConfig {
        HoneytokenConfig::builder(KeySource::Bytes(test_key()))
            .pool_size(pool_size)
            .build()
    }

    fn make_store(pool_size: usize) -> HoneytokenStore {
        HoneytokenStore::new(test_config(pool_size)).expect("store creation should succeed")
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let store = make_store(5);
        let plaintext = b"super-secret-canary-token";
        let encrypted = store.encrypt_token(plaintext).unwrap();

        assert!(encrypted.len() >= NONCE_LEN + GCM_TAG_LEN + plaintext.len());

        let decrypted = store.decrypt_token(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn pool_generation_correct_count_and_unique() {
        let store = make_store(10);
        let state = store.state.read().unwrap();
        assert_eq!(state.tokens.len(), 10);

        let plaintexts: std::collections::HashSet<&str> =
            state.tokens.iter().map(|t| t.plaintext.as_str()).collect();
        assert_eq!(plaintexts.len(), 10, "all tokens must be unique");
    }

    #[test]
    fn inject_into_prompt_tokens_appear() {
        let store = make_store(10);
        let prompt = "Hello, I am a helpful assistant. How can I help you today?";
        let (modified, ids) = store.inject_into_prompt(prompt).unwrap();

        assert!(!ids.is_empty(), "should inject at least one token");
        let state = store.state.read().unwrap();
        for id in &ids {
            let token = state.tokens.iter().find(|t| &t.id == id).unwrap();
            assert!(
                modified.contains(&*token.plaintext),
                "injected token plaintext should appear in modified prompt"
            );
        }
    }

    #[test]
    fn detect_in_output_finds_tokens() {
        let store = make_store(5);
        let (token_id, leaked) = {
            let state = store.state.read().unwrap();
            let t = &state.tokens[0];
            (
                t.id.clone(),
                format!("The answer is {} and more", &*t.plaintext),
            )
        };

        let detections = store.detect_in_output(&Content::Text(leaked));
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].token_id, token_id);
        assert_eq!(detections[0].severity, Severity::Critical);
        assert!(detections[0].position.start < detections[0].position.end);
    }

    #[test]
    fn rotation_marks_old_detection_only_and_still_detectable() {
        let store = make_store(5);

        let old_plaintext = {
            let state = store.state.read().unwrap();
            state.tokens[0].plaintext.clone()
        };

        store.rotate().unwrap();

        {
            let state = store.state.read().unwrap();
            // 5 old (detection-only) + 5 new (active)
            assert_eq!(state.tokens.len(), 10);
            assert_eq!(state.tokens.iter().filter(|t| t.detection_only).count(), 5);
            assert_eq!(state.tokens.iter().filter(|t| !t.detection_only).count(), 5);
        }

        // Old token must still be detectable
        let output = Content::Text(format!("leaked: {}", &*old_plaintext));
        let detections = store.detect_in_output(&output);
        assert!(
            !detections.is_empty(),
            "rotated-out tokens must still be detectable"
        );
    }

    #[test]
    fn key_too_short_error() {
        let short_key = Zeroizing::new(vec![0u8; 16]);
        let config = HoneytokenConfig::builder(KeySource::Bytes(short_key))
            .pool_size(1)
            .build();
        let err = HoneytokenStore::new(config).unwrap_err();
        assert!(
            matches!(
                err,
                HoneytokenError::KeyTooShort {
                    expected: 32,
                    actual: 16
                }
            ),
            "expected KeyTooShort, got: {err:?}"
        );
    }

    #[test]
    fn random_text_no_false_positives() {
        let store = make_store(50);
        let benign = Content::Text(
            "The quick brown fox jumps over the lazy dog. Nothing suspicious here.".into(),
        );
        let detections = store.detect_in_output(&benign);
        assert!(
            detections.is_empty(),
            "no false positives expected: {detections:?}"
        );
    }

    #[test]
    fn debug_impl_does_not_show_plaintext() {
        let store = make_store(3);
        let state = store.state.read().unwrap();
        let actual_plaintext = state.tokens[0].plaintext.clone();
        let debug_str = format!("{:?}", state.tokens[0]);

        assert!(
            debug_str.contains("[REDACTED]"),
            "Debug output should contain [REDACTED]"
        );
        assert!(
            !debug_str.contains(&*actual_plaintext),
            "Debug output must NOT contain plaintext"
        );
    }

    #[test]
    fn hmac_fingerprint_deterministic() {
        let store = make_store(3);
        let state = store.state.read().unwrap();
        let token = &state.tokens[0];

        // Re-sign the same plaintext with the same HMAC key
        let tag = hmac::sign(&store.hmac_key, token.plaintext.as_bytes());
        let expected = hex_encode(tag.as_ref());

        assert_eq!(
            token.hmac_fingerprint, expected,
            "HMAC fingerprint must be deterministic for same plaintext and key"
        );
    }
}
