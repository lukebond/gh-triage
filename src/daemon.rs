use crate::config::{Config, NotifyOn};
use crate::db::Db;
use crate::github::GithubClient;
use crate::notify::send_notification;
use crate::summary::generate_summary;
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

    // Collect items that need summaries
    let mut needs_summary: Vec<(String, String, String, String)> = Vec::new();

    for (item, _reason) in &results {
        let (inserted, updated) = db.upsert_item(item)?;
        if inserted {
            new_count += 1;
            if !is_first_poll {
                send_notification(&config.notify_urgency, &item.repo, &item.title);
            }
            // Queue summary generation
            let body = item.body.clone().unwrap_or_default();
            needs_summary.push((
                item.id.clone(),
                item.item_type.as_str().to_string(),
                item.title.clone(),
                body,
            ));
        } else if updated && config.notify_on == NotifyOn::NewActivity {
            updated_count += 1;
            if !is_first_poll {
                send_notification(&config.notify_urgency, &item.repo, &item.title);
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
    for (id, item_type, title, body) in needs_summary {
        let db_path = db_path_owned.clone();
        tokio::spawn(async move {
            if let Some(summary) = generate_summary(&item_type, &title, &body).await {
                if let Ok(db) = Db::open(&db_path) {
                    let _ = db.set_summary(&id, &summary);
                }
            }
        });
    }

    Ok(())
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
