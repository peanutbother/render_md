//! Warnings collected while transforming the vault. Problems that make a
//! page wrong (a link or embed that doesn't resolve, a missing heading or
//! block, an embed cycle) are [`Severity::Broken`] and fail `--strict`;
//! everything else is informational.

use std::collections::BTreeSet;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Broken,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Vault-relative path of the note (or file) the problem is in, empty
    /// for problems of the whole vault.
    pub source: String,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Broken => "broken",
            Severity::Warning => "warning",
        };
        if self.source.is_empty() {
            write!(f, "{level}: {}", self.message)
        } else {
            write!(f, "{level}: {}: {}", self.source, self.message)
        }
    }
}

/// A de-duplicated, sorted set of diagnostics.
#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    items: BTreeSet<Diagnostic>,
}

impl Diagnostics {
    pub fn broken(&mut self, source: &str, message: impl Into<String>) {
        self.push(Severity::Broken, source, message.into());
    }

    pub fn warn(&mut self, source: &str, message: impl Into<String>) {
        self.push(Severity::Warning, source, message.into());
    }

    fn push(&mut self, severity: Severity, source: &str, message: String) {
        self.items.insert(Diagnostic {
            severity,
            source: source.to_owned(),
            message,
        });
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn broken_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.severity == Severity::Broken)
            .count()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
