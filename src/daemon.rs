use crate::config::{Config, NotifyOn};
use crate::db::Db;
use crate::github::GithubClient;
use crate::notify::send_notification;
use crate::summary::{generate_summary, generate_update_summary};
use chrono::Utc;
use std::path::Path;

/// Run one poll cycle: fetch from GitHub, upsert into DB, notify, generate summaries.
pub async fn run_poll(config: &Config, db_path: &Path) -> Result<(), crate::AppError> {
    let db = Db::open(db_path)?;
    let client = GithubClient::new(&config.github_token);
    let last_poll = db.get_last_poll()?;

    eprintln!("[poll] starting poll at {}", Utc::now().format("%H:%M:%S"));

    let results = client.poll(config, last_poll).await?;
    let mut new_count = 0;
    let mut updated_count = 0;
    let is_first_poll = last_poll.is_none();

    // Collect items that need summaries: (id, item_type, title, body, url, prev_comment_count, new_comment_count)
    let mut needs_summary: Vec<SummaryJob> = Vec::new();

    for (item, reason) in &results {
        let (inserted, updated, prev_comment_count) = db.upsert_item(item)?;
        let reason_label = match reason.as_str() {
            "review_requested" => "Review requested",
            "assigned" => "Assigned",
            "authored" => "Authored",
            "mentioned" => "Mentioned",
            "all" => "Activity",
            other => other,
        };
        if inserted {
            new_count += 1;
            if !is_first_poll {
                let body = format!("[{reason_label}] {}", item.title);
                send_notification(&config.notify_urgency, &item.repo, &body);
            }
            needs_summary.push(SummaryJob {
                id: item.id.clone(),
                item_type: item.item_type.as_str().to_string(),
                title: item.title.clone(),
                body: item.body.clone().unwrap_or_default(),
                url: item.url.clone(),
                prev_comment_count: 0,
                new_comment_count: item.comment_count,
                is_update: false,
            });
        } else if updated {
            updated_count += 1;
            if !is_first_poll && config.notify_on == NotifyOn::NewActivity {
                let body = format!("[{reason_label}] {}", item.title);
                send_notification(&config.notify_urgency, &item.repo, &body);
            }
            // Only regenerate summary if comment count increased
            if item.comment_count > prev_comment_count {
                needs_summary.push(SummaryJob {
                    id: item.id.clone(),
                    item_type: item.item_type.as_str().to_string(),
                    title: item.title.clone(),
                    body: item.body.clone().unwrap_or_default(),
                    url: item.url.clone(),
                    prev_comment_count,
                    new_comment_count: item.comment_count,
                    is_update: true,
                });
            }
        }
    }

    db.set_last_poll(Utc::now())?;
    eprintln!(
        "[poll] done: {} new, {} updated, {} total results",
        new_count,
        updated_count,
        results.len()
    );

    // Generate summaries in background
    let db_path_owned = db_path.to_path_buf();
    let token = config.github_token.clone();
    for job in needs_summary {
        let db_path = db_path_owned.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let summary = if job.is_update {
                // Fetch only the new comments
                let num_new = (job.new_comment_count - job.prev_comment_count) as usize;
                let client = GithubClient::new(&token);
                match client.fetch_recent_comments(&job.url, num_new).await {
                    Ok(comments) => {
                        let new_comments: Vec<(String, String)> = comments
                            .into_iter()
                            .map(|c| (c.user.login, c.body.unwrap_or_default()))
                            .collect();
                        generate_update_summary(
                            &job.item_type,
                            &job.title,
                            &job.body,
                            &new_comments,
                        )
                        .await
                    }
                    Err(e) => {
                        eprintln!("[summary] failed to fetch comments for {}: {e}", job.url);
                        None
                    }
                }
            } else {
                generate_summary(&job.item_type, &job.title, &job.body).await
            };

            if let Some(summary) = summary {
                if let Ok(db) = Db::open(&db_path) {
                    let _ = db.set_summary(&job.id, &summary);
                }
            }
        });
    }

    Ok(())
}

struct SummaryJob {
    id: String,
    item_type: String,
    title: String,
    body: String,
    url: String,
    prev_comment_count: u32,
    new_comment_count: u32,
    is_update: bool,
}

/// Run the daemon loop: poll, sleep, repeat.
pub async fn run_daemon(config: Config, db_path: &Path) -> Result<(), crate::AppError> {
    let interval = config.poll_interval;
    loop {
        if let Err(e) = run_poll(&config, db_path).await {
            eprintln!("[daemon] poll error: {e}");
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
    }
}
