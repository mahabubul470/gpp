//! `gpp-remote` — platform integration (layer 17).
//!
//! gpp treats GitHub/GitLab/Bitbucket as first-class *sync targets*, not
//! competitors. This crate:
//!
//! * reads `[remote]` config from `.gpp/remote/config.toml`,
//! * builds an **enriched PR body** from gpp metadata (intent, semantic
//!   summary, cost, policy, trust) — pure and tested,
//! * creates PRs via each platform's REST API through an injected
//!   [`HttpClient`] (so dispatch is unit-testable offline),
//! * pushes plain Git via the [`gpp_git_bridge`] export + `git push`
//!   ([`GenericGitRemote`]) so Git history stays clean.
//!
//! See `docs/CLI_SPEC.md` (§ gpp remote), `docs/ROADMAP.md` (Phase 7).
#![forbid(unsafe_code)]

use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("git error: {0}")]
    Git(String),
    #[error("unknown platform {0:?}")]
    UnknownPlatform(String),
}

pub type Result<T> = std::result::Result<T, Error>;

// ---- config ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    GitHub,
    GitLab,
    Bitbucket,
    Generic,
}

impl Platform {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "github" => Platform::GitHub,
            "gitlab" => Platform::GitLab,
            "bitbucket" => Platform::Bitbucket,
            "generic" => Platform::Generic,
            other => return Err(Error::UnknownPlatform(other.to_string())),
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::GitHub => "github",
            Platform::GitLab => "gitlab",
            Platform::Bitbucket => "bitbucket",
            Platform::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawConfig {
    #[serde(default = "default_platform")]
    platform: String,
    #[serde(default = "default_token_env")]
    api_token_env: String,
    #[serde(default)]
    repository: String,
    #[serde(default)]
    remote_url: String,
}

fn default_platform() -> String {
    "generic".into()
}
fn default_token_env() -> String {
    "GITHUB_TOKEN".into()
}

#[derive(Debug, Clone)]
pub struct RemoteConfig {
    pub platform: Platform,
    pub api_token_env: String,
    pub repository: String,
    pub remote_url: String,
}

impl RemoteConfig {
    /// Parse the `[remote]` table of a `.gpp/config.toml` (or a dedicated
    /// `.gpp/remote/config.toml`).
    pub fn load(gpp_dir: &Path) -> Result<RemoteConfig> {
        let dedicated = gpp_dir.join("remote").join("config.toml");
        let text = if dedicated.exists() {
            std::fs::read_to_string(dedicated)?
        } else {
            let main = std::fs::read_to_string(gpp_dir.join("config.toml"))?;
            // Extract just the [remote] table.
            let v: toml::Value = main.parse().map_err(|e| Error::Config(format!("{e}")))?;
            v.get("remote")
                .map(|r| toml::to_string(r).unwrap_or_default())
                .unwrap_or_default()
        };
        let raw: RawConfig = toml::from_str(&text).map_err(|e| Error::Config(format!("{e}")))?;
        Ok(RemoteConfig {
            platform: Platform::parse(&raw.platform)?,
            api_token_env: raw.api_token_env,
            repository: raw.repository,
            remote_url: raw.remote_url,
        })
    }

    pub fn save(&self, gpp_dir: &Path) -> Result<()> {
        let dir = gpp_dir.join("remote");
        std::fs::create_dir_all(&dir)?;
        let body = format!(
            "platform = {:?}\napi_token_env = {:?}\nrepository = {:?}\nremote_url = {:?}\n",
            self.platform.as_str(),
            self.api_token_env,
            self.repository,
            self.remote_url
        );
        std::fs::write(dir.join("config.toml"), body)?;
        Ok(())
    }
}

// ---- PR enrichment (pure) --------------------------------------------------

/// gpp metadata folded into a PR/MR description.
#[derive(Debug, Clone, Default)]
pub struct Enrichment {
    pub intent: Option<String>,
    pub semantic_summary: Vec<String>,
    pub agent: Option<String>,
    pub policy_results: Vec<String>,
    pub cost_usd: Option<f64>,
    pub trust: Option<(String, f64)>, // (agent_id, score)
}

/// Build a Markdown PR body: the human title/message, then a gpp section.
pub fn pr_body(title: &str, message: &str, e: &Enrichment) -> String {
    let mut s = String::new();
    s.push_str(message.trim());
    s.push_str("\n\n---\n### 🤖 gpp metadata\n");
    if let Some(i) = &e.intent {
        s.push_str(&format!("- **Intent:** {i}\n"));
    }
    if let Some(a) = &e.agent {
        s.push_str(&format!("- **Agent:** {a}\n"));
    }
    if let Some((id, score)) = &e.trust {
        s.push_str(&format!("- **Trust:** {id} ({score:.1})\n"));
    }
    if let Some(c) = e.cost_usd {
        s.push_str(&format!("- **Cost:** ${c:.4}\n"));
    }
    if !e.policy_results.is_empty() {
        s.push_str("- **Policy:**\n");
        for p in &e.policy_results {
            s.push_str(&format!("  - {p}\n"));
        }
    }
    if !e.semantic_summary.is_empty() {
        s.push_str("- **Semantic changes:**\n");
        for c in &e.semantic_summary {
            s.push_str(&format!("  - `{c}`\n"));
        }
    }
    s.push_str(&format!("\n_PR title: {title}_\n"));
    s
}

// ---- platform abstraction --------------------------------------------------

#[derive(Debug, Clone)]
pub struct PrRequest {
    pub title: String,
    pub body: String,
    pub head: String,
    pub base: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrResult {
    pub number: u64,
    pub url: String,
}

/// One outbound HTTP request (method is always POST here).
pub struct HttpRequest<'a> {
    pub url: String,
    pub token: &'a str,
    pub body: String,
}

/// Abstracts the HTTP POST so PR creation is unit-testable offline.
pub trait HttpClient {
    /// Returns `(status, response_body)`.
    fn post_json(&self, req: &HttpRequest) -> Result<(u16, String)>;
}

/// Real client: blocking `reqwest` with platform auth headers.
pub struct ReqwestClient {
    pub auth_header: &'static str, // "Authorization" | "PRIVATE-TOKEN"
    pub bearer: bool,
}

impl HttpClient for ReqwestClient {
    fn post_json(&self, req: &HttpRequest) -> Result<(u16, String)> {
        let client = reqwest::blocking::Client::new();
        let token_val = if self.bearer {
            format!("Bearer {}", req.token)
        } else {
            req.token.to_string()
        };
        let resp = client
            .post(&req.url)
            .header("User-Agent", "gpp-remote")
            .header("Accept", "application/json")
            .header(self.auth_header, token_val)
            .header("Content-Type", "application/json")
            .body(req.body.clone())
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.text().map_err(|e| Error::Http(e.to_string()))?;
        Ok((status, text))
    }
}

/// Create a PR/MR on a platform. `repo` is `owner/name`.
pub fn create_pr(
    platform: Platform,
    repo: &str,
    token: &str,
    pr: &PrRequest,
    http: &dyn HttpClient,
) -> Result<PrResult> {
    let (url, body) = match platform {
        Platform::GitHub => (
            format!("https://api.github.com/repos/{repo}/pulls"),
            serde_json::json!({
                "title": pr.title, "body": pr.body,
                "head": pr.head, "base": pr.base,
            }),
        ),
        Platform::GitLab => (
            format!(
                "https://gitlab.com/api/v4/projects/{}/merge_requests",
                urlencode(repo)
            ),
            serde_json::json!({
                "title": pr.title, "description": pr.body,
                "source_branch": pr.head, "target_branch": pr.base,
            }),
        ),
        Platform::Bitbucket => (
            format!("https://api.bitbucket.org/2.0/repositories/{repo}/pullrequests"),
            serde_json::json!({
                "title": pr.title,
                "description": pr.body,
                "source": {"branch": {"name": pr.head}},
                "destination": {"branch": {"name": pr.base}},
            }),
        ),
        Platform::Generic => {
            return Err(Error::Platform(
                "generic platform has no PR API — use `gpp remote push`".into(),
            ));
        }
    };
    let (status, resp) = http.post_json(&HttpRequest {
        url,
        token,
        body: body.to_string(),
    })?;
    if !(200..300).contains(&status) {
        return Err(Error::Platform(format!("HTTP {status}: {resp}")));
    }
    parse_pr_result(platform, &resp)
}

fn parse_pr_result(platform: Platform, resp: &str) -> Result<PrResult> {
    let v: serde_json::Value =
        serde_json::from_str(resp).map_err(|e| Error::Platform(e.to_string()))?;
    let (num, url) = match platform {
        Platform::GitHub => (v.get("number"), v.get("html_url")),
        Platform::GitLab => (v.get("iid"), v.get("web_url")),
        Platform::Bitbucket => (
            v.get("id"),
            v.get("links")
                .and_then(|l| l.get("html"))
                .and_then(|h| h.get("href")),
        ),
        Platform::Generic => (None, None),
    };
    Ok(PrResult {
        number: num.and_then(|n| n.as_u64()).unwrap_or(0),
        url: url.and_then(|u| u.as_str()).unwrap_or("").to_string(),
    })
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---- generic git remote ----------------------------------------------------

/// Export gpp history to a scratch Git repo and `git push` it to a plain
/// Git remote URL — no platform API, clean Git history only.
pub struct GenericGitRemote;

impl GenericGitRemote {
    /// `gpp_dir` is `.gpp/`; `remote_url` a Git URL; `branch` to push.
    pub fn push(gpp_dir: &Path, remote_url: &str, branch: &str) -> Result<String> {
        let scratch = std::env::temp_dir().join(format!(
            "gpp-export-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_micros())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&scratch)?;
        gpp_git_bridge::export(gpp_dir, &scratch)
            .map_err(|e| Error::Git(format!("export: {e}")))?;
        let out = std::process::Command::new("git")
            .args(["-C", &scratch.display().to_string()])
            .args(["push", remote_url, &format!("{branch}:{branch}"), "--force"])
            .output()
            .map_err(|e| Error::Git(e.to_string()))?;
        let _ = std::fs::remove_dir_all(&scratch);
        if !out.status.success() {
            return Err(Error::Git(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        Ok(format!("pushed {branch} → {remote_url}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn enriched_body_has_human_message_then_metadata() {
        let e = Enrichment {
            intent: Some("Feature".into()),
            semantic_summary: vec!["+ fn handle_retry".into()],
            agent: Some("agent:claude".into()),
            policy_results: vec!["secrets-scan: pass".into()],
            cost_usd: Some(0.0123),
            trust: Some(("agent:claude".into(), 94.2)),
        };
        let body = pr_body("Add retry queue", "Implements exponential backoff.", &e);
        assert!(body.starts_with("Implements exponential backoff."));
        assert!(body.contains("### 🤖 gpp metadata"));
        assert!(body.contains("**Intent:** Feature"));
        assert!(body.contains("**Trust:** agent:claude (94.2)"));
        assert!(body.contains("$0.0123"));
        assert!(body.contains("`+ fn handle_retry`"));
    }

    #[test]
    fn config_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();
        RemoteConfig {
            platform: Platform::GitHub,
            api_token_env: "GH_TOKEN".into(),
            repository: "acme/webapp".into(),
            remote_url: "git@github.com:acme/webapp.git".into(),
        }
        .save(&gpp)
        .unwrap();
        let c = RemoteConfig::load(&gpp).unwrap();
        assert_eq!(c.platform, Platform::GitHub);
        assert_eq!(c.repository, "acme/webapp");
        assert_eq!(c.api_token_env, "GH_TOKEN");
    }

    struct MockHttp {
        seen: RefCell<Option<HttpRequest<'static>>>,
        status: u16,
        resp: String,
    }
    impl HttpClient for MockHttp {
        fn post_json(&self, req: &HttpRequest) -> Result<(u16, String)> {
            *self.seen.borrow_mut() = Some(HttpRequest {
                url: req.url.clone(),
                token: "redacted",
                body: req.body.clone(),
            });
            Ok((self.status, self.resp.clone()))
        }
    }

    #[test]
    fn github_create_pr_parses_result() {
        let m = MockHttp {
            seen: RefCell::new(None),
            status: 201,
            resp: r#"{"number":42,"html_url":"https://github.com/acme/webapp/pull/42"}"#.into(),
        };
        let r = create_pr(
            Platform::GitHub,
            "acme/webapp",
            "tok",
            &PrRequest {
                title: "t".into(),
                body: "b".into(),
                head: "feature".into(),
                base: "main".into(),
            },
            &m,
        )
        .unwrap();
        assert_eq!(r.number, 42);
        assert_eq!(r.url, "https://github.com/acme/webapp/pull/42");
        let seen = m.seen.borrow();
        assert!(
            seen.as_ref()
                .unwrap()
                .url
                .ends_with("/repos/acme/webapp/pulls")
        );
    }

    #[test]
    fn gitlab_uses_encoded_project_and_iid() {
        let m = MockHttp {
            seen: RefCell::new(None),
            status: 201,
            resp: r#"{"iid":7,"web_url":"https://gitlab.com/acme/webapp/-/merge_requests/7"}"#
                .into(),
        };
        let r = create_pr(
            Platform::GitLab,
            "acme/webapp",
            "tok",
            &PrRequest {
                title: "t".into(),
                body: "b".into(),
                head: "f".into(),
                base: "main".into(),
            },
            &m,
        )
        .unwrap();
        assert_eq!(r.number, 7);
        assert!(
            m.seen
                .borrow()
                .as_ref()
                .unwrap()
                .url
                .contains("acme%2Fwebapp")
        );
    }

    #[test]
    fn http_error_is_surfaced() {
        let m = MockHttp {
            seen: RefCell::new(None),
            status: 422,
            resp: r#"{"message":"validation failed"}"#.into(),
        };
        let err = create_pr(
            Platform::GitHub,
            "r/x",
            "t",
            &PrRequest {
                title: "t".into(),
                body: "b".into(),
                head: "h".into(),
                base: "main".into(),
            },
            &m,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("422"));
    }

    #[test]
    fn generic_has_no_pr_api() {
        struct Never;
        impl HttpClient for Never {
            fn post_json(&self, _: &HttpRequest) -> Result<(u16, String)> {
                panic!("must not be called")
            }
        }
        assert!(
            create_pr(
                Platform::Generic,
                "r",
                "t",
                &PrRequest {
                    title: "t".into(),
                    body: "b".into(),
                    head: "h".into(),
                    base: "m".into()
                },
                &Never
            )
            .is_err()
        );
    }
}
