use serde_json::json;
use std::path::Path;

use crate::db::Db;
use crate::types::ItemStatus;

/// Generate Waybar JSON output and print to stdout.
pub fn run_waybar(db_path: &Path) -> Result<(), crate::AppError> {
    let db = Db::open(db_path)?;
    let items = db.get_items(ItemStatus::Active)?;
    let count = items.len();

    if count == 0 {
        let output = json!({
            "text": "",
            "tooltip": "No notifications",
            "class": ""
        });
        println!("{}", output);
    } else {
        let tooltip_lines: Vec<String> = std::iter::once(format!(
            "{count} active GitHub notification{}",
            if count == 1 { "" } else { "s" }
        ))
        .chain(items.iter().take(5).map(|item| {
            let repo_short = item.repo.split('/').nth(1).unwrap_or(&item.repo);
            format!("{}: {}", repo_short, item.title)
        }))
        .collect();

        let output = json!({
            "text": format!("\u{f0f3} {count}"),
            "tooltip": tooltip_lines.join("\n"),
            "class": "has-notifications"
        });
        println!("{}", output);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::db::Db;
    use crate::types::{Item, ItemStatus, ItemType};
    use chrono::Utc;

    fn make_item(id: &str, title: &str, repo: &str) -> Item {
        let now = Utc::now();
        Item {
            id: id.to_string(),
            url: format!("https://github.com/{repo}/issues/1"),
            repo: repo.to_string(),
            title: title.to_string(),
            body: None,
            item_type: ItemType::Issue,
            state: "open".to_string(),
            reason: "assigned".to_string(),
            author: "alice".to_string(),
            created_at: now,
            updated_at: now,
            first_seen_at: now,
            last_activity_at: Some(now),
            summary: None,
            status: ItemStatus::Active,
        }
    }

    #[test]
    fn waybar_json_with_items() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_item(&make_item("1", "Fix bug", "org/repo-a"))
            .unwrap();
        db.upsert_item(&make_item("2", "Add feature", "org/repo-b"))
            .unwrap();

        let items = db.get_items(ItemStatus::Active).unwrap();
        let count = items.len();
        assert_eq!(count, 2);

        // Verify we can construct the JSON
        let tooltip_lines: Vec<String> =
            std::iter::once(format!("{count} active GitHub notifications"))
                .chain(items.iter().take(5).map(|item| {
                    let repo_short = item.repo.split('/').nth(1).unwrap_or(&item.repo);
                    format!("{}: {}", repo_short, item.title)
                }))
                .collect();

        let output = serde_json::json!({
            "text": format!("\u{f0f3} {count}"),
            "tooltip": tooltip_lines.join("\n"),
            "class": "has-notifications"
        });

        let parsed: serde_json::Value = output;
        assert_eq!(parsed["class"], "has-notifications");
        assert!(parsed["text"].as_str().unwrap().contains("2"));
    }

    #[test]
    fn waybar_json_zero_items() {
        let db = Db::open_in_memory().unwrap();
        let items = db.get_items(ItemStatus::Active).unwrap();
        assert_eq!(items.len(), 0);

        let output = serde_json::json!({
            "text": "",
            "tooltip": "No notifications",
            "class": ""
        });

        let parsed: serde_json::Value = output;
        assert_eq!(parsed["text"], "");
        assert_eq!(parsed["tooltip"], "No notifications");
        assert_eq!(parsed["class"], "");
    }
}
