use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use std::collections::HashMap;
use thiserror::Error;

use crate::config::Config;
use crate::types::{Comment, Item, ItemStatus, SearchItem, SearchResponse};

#[derive(Error, Debug)]
pub enum GithubError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("GitHub API rate limited, retry after backoff")]
    RateLimited,
    #[error("GitHub API error: {status} {body}")]
    ApiError { status: u16, body: String },
}

pub struct GithubClient {
    client: reqwest::Client,
    token: String,
}

impl GithubClient {
    pub fn new(token: &str) -> Self {
        GithubClient {
            client: reqwest::Client::new(),
            token: token.to_string(),
        }
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchItem>, GithubError> {
        let url = format!(
            "https://api.github.com/search/issues?q={}&per_page=100",
            urlencoding(query)
        );
        let resp = self
            .client
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(USER_AGENT, "gh-triage")
            .header(ACCEPT, "application/vnd.github+json")
            .send()
            .await?;

        // Check rate limit
        if let Some(remaining) = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok())
        {
            if remaining == 0 {
                return Err(GithubError::RateLimited);
            }
        }

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GithubError::ApiError {
                status: status.as_u16(),
                body,
            });
        }

        let search_resp: SearchResponse = resp.json().await?;
        Ok(search_resp.items)
    }

    /// Fetch recent comments on an issue or PR.
    /// `url` is an HTML URL like https://github.com/owner/repo/issues/123
    /// Returns the last `limit` comments in chronological order.
    pub async fn fetch_recent_comments(
        &self,
        url: &str,
        limit: usize,
    ) -> Result<Vec<Comment>, GithubError> {
        // Convert HTML URL to API comments endpoint
        let api_url = url
            .replace("https://github.com/", "https://api.github.com/repos/")
            .replace("/pull/", "/issues/")
            + "/comments";
        let api_url = format!("{api_url}?per_page={limit}&page=1&direction=desc");

        let resp = self
            .client
            .get(&api_url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(USER_AGENT, "gh-triage")
            .header(ACCEPT, "application/vnd.github+json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GithubError::ApiError {
                status: status.as_u16(),
                body,
            });
        }

        let mut comments: Vec<Comment> = resp.json().await?;
        // API returns newest first with direction=desc, reverse to chronological
        comments.reverse();
        Ok(comments)
    }

    /// Run all queries for a poll cycle and return deduplicated items.
    pub async fn poll(
        &self,
        config: &Config,
        last_poll: Option<DateTime<Utc>>,
    ) -> Result<Vec<(Item, String)>, GithubError> {
        let mut all_items: HashMap<String, (SearchItem, String)> = HashMap::new();
        let org = &config.github_org;
        let user = &config.github_user;

        // "for_me" queries
        let queries_for_me = vec![
            (
                format!("review-requested:{user} org:{org} is:open"),
                "review_requested",
            ),
            (format!("assignee:{user} org:{org} is:open"), "assigned"),
            (format!("author:{user} org:{org} is:open"), "authored"),
            (format!("mentions:{user} org:{org} is:open"), "mentioned"),
        ];

        for (query, reason) in &queries_for_me {
            let items = self.search(query).await?;
            for item in items {
                // Skip ignored repos
                let repo = item.repo_name();
                if config.repos_ignore.contains(&repo) {
                    continue;
                }
                // Skip items from "all" repos (they'll be fetched separately)
                if config.repos_all.contains(&repo) {
                    continue;
                }
                all_items
                    .entry(item.html_url.clone())
                    .or_insert((item, reason.to_string()));
            }
        }

        // Also check recently closed for_me items
        if let Some(last) = last_poll {
            let since = last.format("%Y-%m-%dT%H:%M:%S").to_string();
            let closed_queries = vec![
                format!("review-requested:{user} org:{org} is:closed updated:>{since}"),
                format!("assignee:{user} org:{org} is:closed updated:>{since}"),
                format!("author:{user} org:{org} is:closed updated:>{since}"),
                format!("mentions:{user} org:{org} is:closed updated:>{since}"),
            ];
            for query in &closed_queries {
                let items = self.search(query).await?;
                for item in items {
                    let repo = item.repo_name();
                    if config.repos_ignore.contains(&repo) || config.repos_all.contains(&repo) {
                        continue;
                    }
                    all_items
                        .entry(item.html_url.clone())
                        .or_insert((item, "mentioned".to_string()));
                }
            }
        }

        // "all" repos
        for repo in &config.repos_all {
            let query = format!("repo:{repo}");
            let items = self.search(&query).await?;
            for item in items {
                all_items
                    .entry(item.html_url.clone())
                    .or_insert((item, "all".to_string()));
            }
        }

        let now = Utc::now();
        let result = all_items
            .into_values()
            .map(|(search_item, reason)| {
                let item = Item {
                    id: search_item.node_id.clone(),
                    url: search_item.html_url.clone(),
                    repo: search_item.repo_name(),
                    title: search_item.title.clone(),
                    body: search_item.body.clone(),
                    item_type: search_item.item_type(),
                    state: search_item.state.clone(),
                    reason: reason.clone(),
                    author: search_item.user.login.clone(),
                    created_at: search_item.created_at,
                    updated_at: search_item.updated_at,
                    first_seen_at: now,
                    last_activity_at: Some(search_item.updated_at),
                    comment_count: search_item.comments.unwrap_or(0),
                    summary: None,
                    status: ItemStatus::Active,
                };
                (item, reason)
            })
            .collect();

        Ok(result)
    }
}

/// Simple URL encoding for query strings.
fn urlencoding(s: &str) -> String {
    s.replace(' ', "+")
        .replace(':', "%3A")
        .replace('@', "%40")
        .replace('>', "%3E")
        .replace('{', "%7B")
        .replace('}', "%7D")
}

/// Build search queries for testing.
#[cfg(test)]
pub fn build_for_me_queries(user: &str, org: &str) -> Vec<String> {
    vec![
        format!("review-requested:{user} org:{org} is:open"),
        format!("assignee:{user} org:{org} is:open"),
        format!("author:{user} org:{org} is:open"),
        format!("mentions:{user} org:{org} is:open"),
    ]
}

#[cfg(test)]
pub fn build_all_query(repo: &str) -> String {
    format!("repo:{repo}")
}

/// Deduplicate items by URL, keeping the first occurrence.
#[cfg(test)]
pub fn deduplicate(items: Vec<SearchItem>) -> Vec<SearchItem> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.html_url.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PullRequestRef, SearchUser};

    #[test]
    fn for_me_queries() {
        let queries = build_for_me_queries("alice", "myorg");
        assert_eq!(queries.len(), 4);
        assert!(queries[0].contains("review-requested:alice"));
        assert!(queries[0].contains("org:myorg"));
        assert!(queries[1].contains("assignee:alice"));
        assert!(queries[2].contains("author:alice"));
        assert!(queries[3].contains("mentions:alice"));
    }

    #[test]
    fn all_query() {
        let q = build_all_query("myorg/repo");
        assert_eq!(q, "repo:myorg/repo");
    }

    #[test]
    fn dedup_by_url() {
        let items = vec![
            SearchItem {
                node_id: "1".to_string(),
                html_url: "https://github.com/a/b/1".to_string(),
                title: "first".to_string(),
                body: None,
                state: "open".to_string(),
                user: SearchUser {
                    login: "alice".to_string(),
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
                comments: Some(0),
                pull_request: None,
                repository_url: "https://api.github.com/repos/a/b".to_string(),
            },
            SearchItem {
                node_id: "1".to_string(),
                html_url: "https://github.com/a/b/1".to_string(),
                title: "duplicate".to_string(),
                body: None,
                state: "open".to_string(),
                user: SearchUser {
                    login: "alice".to_string(),
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
                comments: Some(0),
                pull_request: None,
                repository_url: "https://api.github.com/repos/a/b".to_string(),
            },
            SearchItem {
                node_id: "2".to_string(),
                html_url: "https://github.com/a/b/2".to_string(),
                title: "second".to_string(),
                body: None,
                state: "open".to_string(),
                user: SearchUser {
                    login: "bob".to_string(),
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
                comments: Some(3),
                pull_request: Some(PullRequestRef {
                    url: "https://api.github.com/repos/a/b/pulls/2".to_string(),
                }),
                repository_url: "https://api.github.com/repos/a/b".to_string(),
            },
        ];
        let deduped = deduplicate(items);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].title, "first");
        assert_eq!(deduped[1].title, "second");
    }
}
