# Incident Response Playbook: LLM01 - Prompt Injection

**OWASP Category**: LLM01:2025 - Prompt Injection  
**MITRE ATLAS**: AML.T0051 (LLM Prompt Injection)  
**Severity**: Critical  
**Last Updated**: January 2026

---

## Overview

Prompt injection attacks attempt to manipulate LLM behavior by injecting malicious instructions into user inputs or retrieved context. This playbook covers detection, containment, eradication, and recovery procedures.

---

## Attack Indicators

### Direct Injection Patterns
- Role manipulation phrases: "Ignore previous instructions", "Forget what I said", "You are now"
- Delimiter confusion: `###SYSTEM###`, `</system><user>`, `---END CONTEXT---`
- Instruction override: "New task:", "Override:", "Disregard security"
- Adversarial suffixes: Unusual token sequences or gibberish with high perplexity

### Indirect Injection Patterns (RAG/Web Content)
- Hidden instructions in documents: white text, tiny fonts, HTML comments
- URL-encoded or base64-encoded commands in metadata
- Markdown abuse: nested code blocks, link labels with instructions
- Multi-stage attacks: benign retrieval followed by malicious follow-up

### Behavioral Indicators
- Sudden change in response tone or format
- Outputting system prompts or configuration details
- Refusing to follow normal instructions
- Generating content outside expected domain
- Honeytoken leakage detected in output

---

## Detection Strategy

### wg-bastion Controls

#### Stage 1: Input Scanner (Heuristic)
```rust
// input/injection.rs
let detector = InjectionScanner::new()
    .with_patterns(InjectionPatterns::default())
    .with_threshold(0.7);

let result = detector.scan(user_input)?;
if result.score > 0.7 {
    // High likelihood of injection
}
```

**Triggers**:
- Known injection phrases (regex + keyword matching)
- Delimiter patterns (role boundary violations)
- Entropy analysis (gibberish detection)
- Unicode tricks (homoglyphs, zero-width chars)

#### Stage 2: ML Classifier (Optional)
```rust
// input/moderation.rs (with moderation-onnx feature)
let classifier = ONNXClassifier::load("models/injection_classifier.onnx")?;
let prediction = classifier.predict(user_input)?;
```

**Triggers**:
- Adversarial suffix detection (trained on attack datasets)
- Context-aware scoring (vs. simple pattern matching)

#### Stage 3: Honeytoken Detection
```rust
// output/egress.rs
let scanner = EgressScanner::new()
    .with_honeytoken_store(store);

let result = scanner.scan(llm_output)?;
if result.honeytoken_leaked.is_some() {
    // IMMEDIATE ALERT - system prompt leaked
}
```

**Triggers**:
- Any honeytoken UUID appears in output
- System prompt fragments in response
- Configuration values in output

### Telemetry Alerts

**SIEM Query** (OpenTelemetry):
```
security.event.type = "prompt_injection_detected" 
AND security.event.severity IN ["high", "critical"]
GROUP BY session_id
HAVING count(*) > 3 IN 5m
```

**Alert Thresholds**:
- Single high-confidence detection → Immediate alert
- 3+ medium-confidence detections in 5 minutes → Escalate
- Honeytoken leak → Critical incident (page on-call)

---

## Containment

### Immediate Actions

**1. Block Request** (if FailMode = Closed)
```rust
// Automated by wg-bastion
if policy.input.fail_mode == FailMode::Closed {
    return Err(PipelineError::ThreatDetected {
        stage: "injection_scanner",
        reason: "Prompt injection detected with score 0.89",
    });
}
```

**2. Isolate Session**
- Mark session_id as suspicious in telemetry
- Increase monitoring sensitivity for this session
- Consider temporary rate limiting for user_id

**3. Prevent Cascading Damage**
- If injection succeeded before detection:
  - Quarantine any generated outputs
  - Do NOT store in agent memory or RAG corpus
  - Flag for human review

**4. Honeytoken Leak Response** (CRITICAL)
```rust
// Automated incident response
if honeytoken_leaked {
    incident_orchestrator.trigger_immediate(IncidentType::SystemPromptLeakage {
        session_id,
        honeytoken_id,
        output_sample: redacted_output,
    });
}
```

Actions:
- Rotate honeytoken immediately
- Audit all recent sessions for similar patterns
- Escalate to security team
- Consider disabling affected prompt template

---

## Eradication

### Root Cause Analysis

**Investigate**:
1. Was this a zero-day injection technique not in pattern DB?
2. Did ML classifier fail to detect (false negative)?
3. Was honeytoken too obvious (reverse-engineered)?
4. Did indirect injection bypass RAG sanitization?

**Update Controls**:
- Add new injection pattern to signature DB
- Retrain ML classifier with attack sample (if corpus updated)
- Rotate honeytokens if pattern detected
- Update RAG sanitization rules

### Pattern Database Update
```rust
// Update injection_patterns.yaml
injection_patterns:
  - pattern: "new attack phrase detected"
    severity: high
    category: direct_injection
    added_date: 2026-01-03
```

### ML Model Retraining (if applicable)
```bash
# Add to adversarial corpus
echo "$attack_sample" >> corpus/prompt_injection/zero_day_$(date +%s).txt

# Trigger retraining (manual or scheduled)
cargo xtask security retrain-classifier --corpus corpus/
```

---

## Recovery

### Service Restoration

**1. Resume Normal Operations**
- If false positive: adjust detection threshold
- If true positive: ensure pattern added to DB
- Lift any temporary rate limits after investigation

**2. User Communication**
- If request blocked: return generic "unsafe content" message
- Do NOT reveal detection methods or patterns
- Provide user-friendly error: "Request could not be processed due to security policy"

**3. Rotate Compromised Secrets** (if honeytoken leaked)
- Generate new honeytoken UUIDs
- Update prompt templates with new markers
- Deploy updated configuration

### Data Cleanup

**Check for Contamination**:
```sql
-- Find sessions with similar injection patterns
SELECT session_id, COUNT(*) as injection_count
FROM security_events
WHERE event_type = 'prompt_injection_detected'
  AND timestamp > NOW() - INTERVAL '24 hours'
GROUP BY session_id
HAVING COUNT(*) > 1;
```

**Quarantine Outputs**:
- Mark affected agent memories as untrusted
- Remove injected content from RAG corpus if stored
- Audit downstream systems that may have received tainted data

---

## Lessons Learned

### Post-Incident Review

**Questions**:
1. How long between injection attempt and detection?
2. Did any malicious content reach end users or downstream systems?
3. Were existing controls effective, or did they fail?
4. Do we need to adjust detection sensitivity?
5. Should we update threat model or playbook procedures?

### Control Improvements

**Short-term** (within 1 week):
- [ ] Add new injection patterns to signature DB
- [ ] Rotate honeytokens if compromised
- [ ] Update telemetry queries with new IOCs
- [ ] Communicate attack pattern to team

**Medium-term** (within 1 month):
- [ ] Retrain ML classifier with new attack samples
- [ ] Review and update prompt fragmentation strategy
- [ ] Conduct red team exercise with similar techniques
- [ ] Update user documentation (if public-facing)

**Long-term** (within 1 quarter):
- [ ] Research emerging injection techniques (academic papers, CVEs)
- [ ] Evaluate new detection technologies (e.g., semantic embeddings)
- [ ] Update threat model and attack trees
- [ ] Share anonymized findings with OWASP community

---

## Escalation Path

| Severity | Escalation | Timeline |
|----------|-----------|----------|
| **Low** (caught by heuristics, no damage) | Log event, monitor session | No escalation |
| **Medium** (caught by ML, suspicious pattern) | Alert security team (Slack) | Within 1 hour |
| **High** (sophisticated zero-day, multiple attempts) | Page on-call, incident response | Immediate |
| **Critical** (honeytoken leak, system prompt extracted) | CEO/CTO notification, external audit | Immediate |

---

## References

- [OWASP LLM01:2025 - Prompt Injection](https://owasp.org/www-project-top-10-for-large-language-model-applications/)
- [MITRE ATLAS AML.T0051](https://atlas.mitre.org/techniques/AML.T0051)
- [Rebuff Prompt Injection Detector](https://github.com/protectai/rebuff)
- [Adversarial Suffix Research (Zou et al.)](https://arxiv.org/abs/2307.15043)
- wg-bastion Threat Model: [../threat_model.md](../threat_model.md)

---

*Last updated: January 2026 - Sprint 1 (WS1-01)*
