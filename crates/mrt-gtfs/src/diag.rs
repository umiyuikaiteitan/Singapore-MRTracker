//! Diagnostics.
//!
//! A diagnostic reports something that the library noticed but did not
//! silently fix: a feed defect, a schedule that cannot be drawn, or a
//! record that a policy excluded. Queries and validation return
//! diagnostics instead of dropping data without a trace.

use serde::{Deserialize, Serialize};

/// How serious a [`Diagnostic`] is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A note. The output is complete.
    Info,
    /// The output is usable, but something is missing or approximate.
    Warning,
    /// The requested output cannot be produced correctly.
    Error,
}

impl Severity {
    /// Get the lowercase name of the severity.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One observation about a feed, a query, or a projection.
///
/// The `code` is a stable machine-readable identifier. Tools may match
/// on it. The `message` is human-readable and may name identifiers
/// from the feed.
///
/// # Examples
///
/// ```
/// use mrt_gtfs::{Diagnostic, Severity};
///
/// let d = Diagnostic::warning("missing-time", "trip T1 has no times");
/// assert_eq!(d.severity, Severity::Warning);
/// assert_eq!(d.to_string(), "warning [missing-time] trip T1 has no times");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// How serious the observation is.
    pub severity: Severity,
    /// A stable machine-readable code, for example `missing-time`.
    ///
    /// The constructors take a `&'static str`, so a code is always a
    /// literal in the source. The field is a `String` so that a
    /// manifest can be read back.
    pub code: String,
    /// A human-readable description.
    pub message: String,
    /// The feed record that the observation is about, if one applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

impl Diagnostic {
    /// Make an informational diagnostic.
    pub fn info(code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Info,
            code: code.to_string(),
            message: message.into(),
            subject: None,
        }
    }

    /// Make a warning diagnostic.
    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            code: code.to_string(),
            message: message.into(),
            subject: None,
        }
    }

    /// Make an error diagnostic.
    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            code: code.to_string(),
            message: message.into(),
            subject: None,
        }
    }

    /// Attach the identifier of the feed record that the diagnostic is
    /// about.
    pub fn about(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}] {}", self.severity, self.code, self.message)?;
        if let Some(subject) = &self.subject {
            write!(f, " ({subject})")?;
        }
        Ok(())
    }
}

/// Sort diagnostics into a stable order and remove exact duplicates.
///
/// The order is by severity (most serious first), then by code, then
/// by subject, then by message. Deterministic output keeps snapshot
/// tests stable.
pub fn normalize(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.code.cmp(&b.code))
            .then_with(|| a.subject.cmp(&b.subject))
            .then_with(|| a.message.cmp(&b.message))
    });
    diagnostics.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_from_mild_to_serious() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn display_names_the_subject() {
        let d = Diagnostic::error("bad-headway", "headway is zero").about("BP_T1");
        assert_eq!(d.to_string(), "error [bad-headway] headway is zero (BP_T1)");
    }

    #[test]
    fn normalize_sorts_and_deduplicates() {
        let mut list = vec![
            Diagnostic::info("b", "second"),
            Diagnostic::error("a", "first"),
            Diagnostic::info("b", "second"),
        ];
        normalize(&mut list);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].severity, Severity::Error);
        assert_eq!(list[1].code, "b");
    }
}
