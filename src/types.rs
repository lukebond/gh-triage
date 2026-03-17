use chrono::{DateTime, Utc};
use serde::Deserialize;

/// A single item (issue or PR) as stored in the local DB.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub url: String,
    pub repo: String,
    pub title: String,
    pub body: Option<String>,
    pub item_type: ItemType,
    pub state: String,
    pub reason: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_seen_at: DateTime<Utc>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub summary: Option<String>,
    pub status: ItemStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemType {
    Issue,
    PullRequest,
}

impl ItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemType::Issue => "issue",
            ItemType::PullRequest => "pull_request",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pull_request" => ItemType::PullRequest,
            _ => ItemType::Issue,
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            ItemType::Issue => "ISS",
            ItemType::PullRequest => "PR ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Active,
    Archived,
}

impl ItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemStatus::Active => "active",
            ItemStatus::Archived => "archived",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "archived" => ItemStatus::Archived,
            _ => ItemStatus::Active,
        }
    }
}

/// GitHub search API response structs.
#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
pub struct SearchItem {
    pub node_id: String,
    pub html_url: String,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub user: SearchUser,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub pull_request: Option<PullRequestRef>,
    pub repository_url: String,
}

impl SearchItem {
    pub fn item_type(&self) -> ItemType {
        if self.pull_request.is_some() {
            ItemType::PullRequest
        } else {
            ItemType::Issue
        }
    }

    /// Extract "owner/repo" from repository_url like "https://api.github.com/repos/owner/repo"
    pub fn repo_name(&self) -> String {
        self.repository_url
            .strip_prefix("https://api.github.com/repos/")
            .unwrap_or(&self.repository_url)
            .to_string()
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchUser {
    pub login: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PullRequestRef {
    pub url: String,
}
