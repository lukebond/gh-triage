use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("config file not found at {0}")]
    NotFound(PathBuf),
    #[error("failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("failed to parse config file: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("no github token configured (set github_token in config or GH_TOKEN env var)")]
    NoToken,
    #[error("missing required field: {0}")]
    MissingField(String),
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    github_token: Option<String>,
    github_user: String,
    github_org: String,
    poll_interval: Option<u64>,
    enter_action: Option<String>,
    notify_urgency: Option<String>,
    notify_on: Option<String>,
    repos: Option<RawRepos>,
}

#[derive(Debug, Deserialize)]
struct RawRepos {
    all: Option<RepoList>,
    ignore: Option<RepoList>,
}

#[derive(Debug, Deserialize)]
struct RepoList {
    repos: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub github_token: String,
    pub github_user: String,
    pub github_org: String,
    pub poll_interval: u64,
    pub enter_action: EnterAction,
    pub notify_urgency: String,
    pub notify_on: NotifyOn,
    pub repos_all: HashSet<String>,
    pub repos_ignore: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnterAction {
    Browser,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyOn {
    NewItem,
    NewActivity,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = config_path();
        if !config_path.exists() {
            return Err(ConfigError::NotFound(config_path));
        }
        let contents = std::fs::read_to_string(&config_path)?;
        Self::parse(&contents)
    }

    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let raw: RawConfig = toml::from_str(contents)?;

        let github_token = raw
            .github_token
            .or_else(|| std::env::var("GH_TOKEN").ok())
            .or_else(gh_cli_token)
            .ok_or(ConfigError::NoToken)?;

        let enter_action = match raw.enter_action.as_deref() {
            Some("preview") => EnterAction::Preview,
            _ => EnterAction::Browser,
        };

        let notify_on = match raw.notify_on.as_deref() {
            Some("new_item") => NotifyOn::NewItem,
            _ => NotifyOn::NewActivity,
        };

        let repos_all = raw
            .repos
            .as_ref()
            .and_then(|r| r.all.as_ref())
            .map(|r| r.repos.iter().cloned().collect())
            .unwrap_or_default();

        let repos_ignore = raw
            .repos
            .as_ref()
            .and_then(|r| r.ignore.as_ref())
            .map(|r| r.repos.iter().cloned().collect())
            .unwrap_or_default();

        Ok(Config {
            github_token,
            github_user: raw.github_user,
            github_org: raw.github_org,
            poll_interval: raw.poll_interval.unwrap_or(120),
            enter_action,
            notify_urgency: raw.notify_urgency.unwrap_or_else(|| "normal".to_string()),
            notify_on,
            repos_all,
            repos_ignore,
        })
    }

    /// Determine the preset for a given "owner/repo" string.
    #[cfg(test)]
    pub fn repo_preset(&self, repo: &str) -> RepoPreset {
        if self.repos_all.contains(repo) {
            RepoPreset::All
        } else if self.repos_ignore.contains(repo) {
            RepoPreset::Ignore
        } else {
            RepoPreset::ForMe
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoPreset {
    All,
    Ignore,
    ForMe,
}

/// Try to get a token from the `gh` CLI.
fn gh_cli_token() -> Option<String> {
    std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gh-triage")
        .join("config.toml")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gh-triage")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_config() {
        std::env::set_var("GH_TOKEN", "test-token");
        let toml = r#"
github_user = "alice"
github_org = "myorg"
poll_interval = 60
enter_action = "browser"
notify_urgency = "low"
notify_on = "new_item"

[repos.all]
repos = ["myorg/repo-a"]

[repos.ignore]
repos = ["myorg/old-repo"]
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.github_user, "alice");
        assert_eq!(cfg.github_org, "myorg");
        assert_eq!(cfg.poll_interval, 60);
        assert_eq!(cfg.enter_action, EnterAction::Browser);
        assert_eq!(cfg.notify_on, NotifyOn::NewItem);
        assert!(cfg.repos_all.contains("myorg/repo-a"));
        assert!(cfg.repos_ignore.contains("myorg/old-repo"));
    }

    #[test]
    fn missing_optional_fields_get_defaults() {
        std::env::set_var("GH_TOKEN", "test-token");
        let toml = r#"
github_user = "bob"
github_org = "org2"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.poll_interval, 120);
        assert_eq!(cfg.enter_action, EnterAction::Browser);
        assert_eq!(cfg.notify_on, NotifyOn::NewActivity);
        assert_eq!(cfg.notify_urgency, "normal");
        assert!(cfg.repos_all.is_empty());
        assert!(cfg.repos_ignore.is_empty());
    }

    #[test]
    fn unknown_repo_defaults_to_for_me() {
        std::env::set_var("GH_TOKEN", "test-token");
        let toml = r#"
github_user = "carol"
github_org = "org3"

[repos.all]
repos = ["org3/special"]
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.repo_preset("org3/special"), RepoPreset::All);
        assert_eq!(cfg.repo_preset("org3/unknown"), RepoPreset::ForMe);
    }
}
