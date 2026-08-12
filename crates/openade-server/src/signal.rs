//! The normalized Signal schema — the wire contract for `POST /signals`.
//!
//! Ported from Merge0's Signal schema v0.7 (MIT — see
//! THIRD_PARTY_NOTICES.md). Any tool can push work into the team inbox by
//! posting this shape; adapters may not invent fields (`deny_unknown_fields`
//! keeps the contract honest — extensions go through this module, never
//! through ad-hoc additions in a sender).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What kind of signal this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Exception,
    UxFriction,
    Ticket,
    Regression,
    Custom,
}

impl SignalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SignalKind::Exception => "exception",
            SignalKind::UxFriction => "ux_friction",
            SignalKind::Ticket => "ticket",
            SignalKind::Regression => "regression",
            SignalKind::Custom => "custom",
        }
    }
}

/// Severity, ordered so `Critical > Low` comparisons work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }
}

/// What an evidence link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Replay,
    StackTrace,
    Ticket,
    Issue,
    Other,
}

/// A deep link into the tool the signal came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceLink {
    pub kind: EvidenceKind,
    /// Human-readable text for the inbox.
    pub label: String,
    /// Deep link to the source tool.
    pub url: String,
}

/// Cross-signal correlation keys. Absent keys are omitted from the
/// serialized form entirely (not `null`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JoinKeys {
    /// Semver or commit SHA.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
    /// Normalized stack location hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_hash: Option<String>,
    /// Vendor-side identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Path component only (no host/query).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_path: Option<String>,
}

/// Why an inbox item was dismissed. The reason is recorded in outcome
/// memory and steers future triage — "intended behavior" reroutes
/// recurrences away from code changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DismissReason {
    IntendedBehavior,
    WontFix,
    Duplicate,
    BadEvidence,
}

impl DismissReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            DismissReason::IntendedBehavior => "intended_behavior",
            DismissReason::WontFix => "wont_fix",
            DismissReason::Duplicate => "duplicate",
            DismissReason::BadEvidence => "bad_evidence",
        }
    }
}

/// What reality decided about an inbox item's work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Merged,
    Closed,
    Reverted,
    Dismissed,
}

impl OutcomeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutcomeKind::Merged => "merged",
            OutcomeKind::Closed => "closed",
            OutcomeKind::Reverted => "reverted",
            OutcomeKind::Dismissed => "dismissed",
        }
    }
}

/// The `POST /signals` wire schema. Senders may not invent fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalIn {
    /// Where the signal came from (free-form in v1: "sentry", "ci", ...).
    pub source: String,
    /// Vendor-native object id, if any.
    #[serde(default)]
    pub source_ref: String,
    pub kind: SignalKind,
    pub severity: Severity,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceLink>,
    /// Stable dedup key; computed from (source, source_ref, kind, title)
    /// when the sender doesn't provide one.
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub join_keys: JoinKeys,
    /// Users/accounts impacted, when the source knows.
    #[serde(default)]
    pub affected_count: Option<i64>,
    /// Original vendor payload, kept verbatim for audit only.
    #[serde(default)]
    pub raw: serde_json::Value,
}

impl SignalIn {
    /// The signal's dedup fingerprint: the sender's, or the computed one.
    pub fn effective_fingerprint(&self) -> String {
        self.fingerprint
            .clone()
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| {
                fingerprint(
                    &self.source,
                    &[&self.source_ref, self.kind.as_str(), &self.title],
                )
            })
    }
}

/// `<source>:<hex16 of sha256>` over length-prefixed parts, so
/// `("ab","c")` can never collide with `("a","bc")`.
pub fn fingerprint(source: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("{source}:{hex}")
}

#[cfg(test)]
#[path = "signal_tests.rs"]
mod tests;
