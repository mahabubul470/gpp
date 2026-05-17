//! `gpp-policy` — compliance-as-code (layer 6).
//!
//! Policies are TOML (`*.policy`) with two rule kinds:
//!
//! * **pattern** — a regex matched against file *content* (secret scanning,
//!   forbidden APIs), optionally scoped by a path glob.
//! * **changeset** — predicates on a changeset (forbid agent authorship,
//!   require human review, cap files touched, forbid touching paths).
//!
//! Each rule has a severity that determines the enforcement point:
//! `audit` (log), `warn` (timeline capture), `block` (stops promotion/sync).
//! Unlike Git hooks, this runs at the storage layer and cannot be skipped.
//!
//! See `docs/SECURITY_MODEL.md`, `docs/ROADMAP.md` (Phase 4).
#![forbid(unsafe_code)]

use std::path::Path;

use globset::{Glob, GlobMatcher};
use regex::Regex;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("policy {file}: {msg}")]
    Parse { file: String, msg: String },
    #[error("invalid regex in rule {rule:?}: {msg}")]
    Regex { rule: String, msg: String },
    #[error("invalid glob {glob:?}: {msg}")]
    Glob { glob: String, msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Audit,
    Warn,
    Block,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Audit => "audit",
            Severity::Warn => "warn",
            Severity::Block => "block",
        }
    }
    fn parse(s: &str) -> Severity {
        match s {
            "block" => Severity::Block,
            "warn" => Severity::Warn,
            _ => Severity::Audit,
        }
    }
}

// ---- on-disk schema (serde) ------------------------------------------------

#[derive(Debug, Deserialize)]
struct PolicyFile {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default, rename = "rule")]
    rules: Vec<RuleFile>,
}

#[derive(Debug, Deserialize)]
struct RuleFile {
    id: String,
    kind: String, // "pattern" | "changeset"
    #[serde(default = "default_sev")]
    severity: String,
    #[serde(default)]
    message: String,
    // pattern rules
    pattern: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    // changeset rules
    #[serde(default)]
    forbid_agent_author: bool,
    #[serde(default)]
    require_human_review: bool,
    max_files: Option<usize>,
    #[serde(default)]
    forbidden_paths: Vec<String>,
}

fn default_sev() -> String {
    "block".into()
}

// ---- compiled forms --------------------------------------------------------

enum Rule {
    Pattern {
        id: String,
        severity: Severity,
        message: String,
        re: Regex,
        scope: Vec<GlobMatcher>,
    },
    Changeset {
        id: String,
        severity: Severity,
        message: String,
        forbid_agent_author: bool,
        require_human_review: bool,
        max_files: Option<usize>,
        forbidden: Vec<GlobMatcher>,
    },
}

/// One policy, compiled and ready to evaluate.
pub struct Policy {
    pub name: String,
    pub description: String,
    rules: Vec<Rule>,
}

/// A single policy violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub policy: String,
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub location: String,
}

/// Facts about a changeset, for changeset-kind rules.
#[derive(Debug, Clone, Default)]
pub struct ChangesetFacts {
    pub author_is_agent: bool,
    pub files: Vec<String>,
    pub has_human_review: bool,
}

fn compile_globs(pats: &[String]) -> Result<Vec<GlobMatcher>> {
    pats.iter()
        .map(|p| {
            Glob::new(p)
                .map(|g| g.compile_matcher())
                .map_err(|e| Error::Glob {
                    glob: p.clone(),
                    msg: e.to_string(),
                })
        })
        .collect()
}

impl Policy {
    /// Parse + compile a `.policy` file. `file` is used only for errors.
    pub fn parse(file: &str, text: &str) -> Result<Policy> {
        let pf: PolicyFile = toml::from_str(text).map_err(|e| Error::Parse {
            file: file.to_string(),
            msg: e.to_string(),
        })?;
        let mut rules = Vec::new();
        for r in pf.rules {
            let severity = Severity::parse(&r.severity);
            let message = if r.message.is_empty() {
                r.id.clone()
            } else {
                r.message.clone()
            };
            match r.kind.as_str() {
                "pattern" => {
                    let pat = r.pattern.ok_or_else(|| Error::Parse {
                        file: file.to_string(),
                        msg: format!("rule {:?} is kind=pattern but has no `pattern`", r.id),
                    })?;
                    let re = Regex::new(&pat).map_err(|e| Error::Regex {
                        rule: r.id.clone(),
                        msg: e.to_string(),
                    })?;
                    rules.push(Rule::Pattern {
                        id: r.id,
                        severity,
                        message,
                        re,
                        scope: compile_globs(&r.files)?,
                    });
                }
                "changeset" => rules.push(Rule::Changeset {
                    id: r.id,
                    severity,
                    message,
                    forbid_agent_author: r.forbid_agent_author,
                    require_human_review: r.require_human_review,
                    max_files: r.max_files,
                    forbidden: compile_globs(&r.forbidden_paths)?,
                }),
                other => {
                    return Err(Error::Parse {
                        file: file.to_string(),
                        msg: format!("rule {:?} has unknown kind {other:?}", r.id),
                    });
                }
            }
        }
        Ok(Policy {
            name: pf.name,
            description: pf.description,
            rules,
        })
    }

    fn check_content(&self, path: &str, content: &str, out: &mut Vec<Violation>) {
        for r in &self.rules {
            if let Rule::Pattern {
                id,
                severity,
                message,
                re,
                scope,
            } = r
            {
                let in_scope = scope.is_empty() || scope.iter().any(|g| g.is_match(path));
                if in_scope && re.is_match(content) {
                    out.push(Violation {
                        policy: self.name.clone(),
                        rule: id.clone(),
                        severity: *severity,
                        message: message.clone(),
                        location: path.to_string(),
                    });
                }
            }
        }
    }

    fn check_changeset(&self, f: &ChangesetFacts, out: &mut Vec<Violation>) {
        for r in &self.rules {
            if let Rule::Changeset {
                id,
                severity,
                message,
                forbid_agent_author,
                require_human_review,
                max_files,
                forbidden,
            } = r
            {
                let mut hit = None;
                if *forbid_agent_author && f.author_is_agent {
                    hit = Some("agent authorship forbidden".to_string());
                } else if *require_human_review && !f.has_human_review {
                    hit = Some("human review required".to_string());
                } else if let Some(m) = max_files
                    && f.files.len() > *m
                {
                    hit = Some(format!("touches {} files (max {m})", f.files.len()));
                } else if let Some(bad) = f
                    .files
                    .iter()
                    .find(|p| forbidden.iter().any(|g| g.is_match(p)))
                {
                    hit = Some(format!("touches forbidden path {bad}"));
                }
                if let Some(why) = hit {
                    out.push(Violation {
                        policy: self.name.clone(),
                        rule: id.clone(),
                        severity: *severity,
                        message: format!("{message} ({why})"),
                        location: "changeset".into(),
                    });
                }
            }
        }
    }
}

/// A collection of active policies.
pub struct PolicySet {
    pub policies: Vec<Policy>,
}

impl PolicySet {
    /// Load every `*.policy` under `dir` (missing dir → empty set).
    pub fn load_dir(dir: &Path) -> Result<PolicySet> {
        let mut policies = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut files: Vec<_> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "policy"))
                .collect();
            files.sort();
            for f in files {
                let text = std::fs::read_to_string(&f)?;
                policies.push(Policy::parse(&f.display().to_string(), &text)?);
            }
        }
        Ok(PolicySet { policies })
    }

    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    /// Evaluate content rules against one file.
    pub fn check_content(&self, path: &str, content: &str) -> Vec<Violation> {
        let mut v = Vec::new();
        for p in &self.policies {
            p.check_content(path, content, &mut v);
        }
        v
    }

    /// Evaluate changeset rules.
    pub fn check_changeset(&self, facts: &ChangesetFacts) -> Vec<Violation> {
        let mut v = Vec::new();
        for p in &self.policies {
            p.check_changeset(facts, &mut v);
        }
        v
    }

    /// The most severe outcome among `violations`, if any.
    pub fn max_severity(violations: &[Violation]) -> Option<Severity> {
        violations.iter().map(|v| v.severity).max()
    }
}

/// Built-in policy templates, installable with `gpp policy add --template`.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("secrets-scan", SECRETS_SCAN),
    ("pci-dss", PCI_DSS),
    ("soc2", SOC2),
];

pub fn template(name: &str) -> Option<&'static str> {
    TEMPLATES.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

const SECRETS_SCAN: &str = r#"name = "secrets-scan"
description = "Detect committed credentials and private keys"

[[rule]]
id = "aws-access-key"
kind = "pattern"
severity = "block"
message = "AWS access key id detected"
pattern = "AKIA[0-9A-Z]{16}"

[[rule]]
id = "private-key-block"
kind = "pattern"
severity = "block"
message = "PEM private key detected"
pattern = "-----BEGIN [A-Z ]*PRIVATE KEY-----"

[[rule]]
id = "generic-secret-assignment"
kind = "pattern"
severity = "warn"
message = "Possible hard-coded secret"
pattern = "(?i)(secret|password|api[_-]?key|token)\\s*[:=]\\s*['\"][^'\"]{8,}['\"]"
"#;

const PCI_DSS: &str = r#"name = "pci-dss"
description = "PCI-DSS guards: PANs in source, review on payment code"

[[rule]]
id = "primary-account-number"
kind = "pattern"
severity = "block"
message = "Possible card PAN (13-16 digits) in source"
pattern = "\\b(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13})\\b"

[[rule]]
id = "payment-code-needs-human-review"
kind = "changeset"
severity = "block"
message = "Changes under payment paths require human review"
require_human_review = true
forbidden_paths = []
files = ["**/payment*/**", "**/billing*/**"]
"#;

const SOC2: &str = r#"name = "soc2"
description = "SOC 2 change-management guards"

[[rule]]
id = "no-direct-prod-config"
kind = "changeset"
severity = "block"
message = "Production config changes must be reviewed by a human"
require_human_review = true
files = ["**/prod/**", "**/production/**"]

[[rule]]
id = "large-changeset-warning"
kind = "changeset"
severity = "warn"
message = "Unusually large changeset"
max_files = 50
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_template_blocks_aws_key() {
        let p = Policy::parse("t", template("secrets-scan").unwrap()).unwrap();
        let set = PolicySet { policies: vec![p] };
        let v = set.check_content("src/config.rs", "let k = \"AKIAIOSFODNN7EXAMPLE\";");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].severity, Severity::Block);
        assert_eq!(v[0].rule, "aws-access-key");
        assert!(set.check_content("clean.rs", "fn ok() {}").is_empty());
    }

    #[test]
    fn changeset_rule_forbids_agent_author() {
        let pol = r#"name="p"
        [[rule]]
        id="no-agent"
        kind="changeset"
        severity="block"
        message="agents may not author here"
        forbid_agent_author=true
        "#;
        let set = PolicySet {
            policies: vec![Policy::parse("p", pol).unwrap()],
        };
        let v = set.check_changeset(&ChangesetFacts {
            author_is_agent: true,
            files: vec!["a.rs".into()],
            has_human_review: false,
        });
        assert_eq!(v.len(), 1);
        assert_eq!(PolicySet::max_severity(&v), Some(Severity::Block));
    }

    #[test]
    fn max_files_and_load_dir() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("soc2.policy"), template("soc2").unwrap()).unwrap();
        std::fs::write(d.path().join("notes.txt"), "ignored").unwrap();
        let set = PolicySet::load_dir(d.path()).unwrap();
        assert_eq!(set.policies.len(), 1);
        let facts = ChangesetFacts {
            author_is_agent: false,
            files: (0..60).map(|i| format!("f{i}.rs")).collect(),
            has_human_review: true,
        };
        let v = set.check_changeset(&facts);
        assert!(v.iter().any(|x| x.rule == "large-changeset-warning"));
    }

    #[test]
    fn invalid_kind_is_rejected() {
        let bad = "name=\"x\"\n[[rule]]\nid=\"r\"\nkind=\"bogus\"\n";
        assert!(Policy::parse("x", bad).is_err());
    }
}
