use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use thiserror::Error;

use crate::types::{Item, ItemStatus, ItemType};

#[derive(Error, Debug)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to create data directory: {0}")]
    CreateDir(std::io::Error),
    #[error("failed to parse datetime from DB: {0}")]
    ChronoParse(#[from] chrono::ParseError),
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS items (
        id TEXT PRIMARY KEY,
        url TEXT NOT NULL,
        repo TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT,
        item_type TEXT NOT NULL,
        state TEXT NOT NULL,
        reason TEXT,
        author TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        first_seen_at TEXT NOT NULL,
        last_activity_at TEXT,
        comment_count INTEGER NOT NULL DEFAULT 0,
        summary TEXT,
        status TEXT NOT NULL DEFAULT 'active'
    );
    CREATE TABLE IF NOT EXISTS meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
";

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(DbError::CreateDir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        // Migrate: add comment_count if missing (for DBs created before this column existed)
        let has_comment_count: bool = conn
            .prepare("SELECT comment_count FROM items LIMIT 0")
            .is_ok();
        if !has_comment_count {
            conn.execute_batch(
                "ALTER TABLE items ADD COLUMN comment_count INTEGER NOT NULL DEFAULT 0;",
            )?;
        }

        Ok(Db { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db { conn })
    }

    /// Insert a new item or update it if the URL already exists and updated_at has changed.
    /// Returns (was_inserted, was_updated, previous_comment_count).
    pub fn upsert_item(&self, item: &Item) -> Result<(bool, bool, u32), DbError> {
        // Check if item exists
        let existing: Option<(String, String, u32)> = self
            .conn
            .query_row(
                "SELECT id, updated_at, comment_count FROM items WHERE id = ?1",
                params![item.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        match existing {
            None => {
                self.conn.execute(
                    "INSERT INTO items (id, url, repo, title, body, item_type, state, reason, author, created_at, updated_at, first_seen_at, last_activity_at, comment_count, summary, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                    params![
                        item.id,
                        item.url,
                        item.repo,
                        item.title,
                        item.body,
                        item.item_type.as_str(),
                        item.state,
                        item.reason,
                        item.author,
                        item.created_at.to_rfc3339(),
                        item.updated_at.to_rfc3339(),
                        item.first_seen_at.to_rfc3339(),
                        item.last_activity_at.map(|d| d.to_rfc3339()),
                        item.comment_count,
                        item.summary,
                        item.status.as_str(),
                    ],
                )?;
                Ok((true, false, 0))
            }
            Some((_id, existing_updated, prev_comment_count)) => {
                let new_updated = item.updated_at.to_rfc3339();
                if existing_updated != new_updated {
                    self.conn.execute(
                        "UPDATE items SET title = ?1, body = ?2, state = ?3, updated_at = ?4, last_activity_at = ?5, reason = ?6, comment_count = ?7, status = 'active' WHERE id = ?8",
                        params![
                            item.title,
                            item.body,
                            item.state,
                            new_updated,
                            item.updated_at.to_rfc3339(),
                            item.reason,
                            item.comment_count,
                            item.id,
                        ],
                    )?;
                    Ok((false, true, prev_comment_count))
                } else {
                    Ok((false, false, prev_comment_count))
                }
            }
        }
    }

    pub fn get_items(&self, status: ItemStatus) -> Result<Vec<Item>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, url, repo, title, body, item_type, state, reason, author,
                    created_at, updated_at, first_seen_at, last_activity_at, comment_count, summary, status
             FROM items WHERE status = ?1 ORDER BY updated_at DESC",
        )?;
        let items = stmt
            .query_map(params![status.as_str()], |row| {
                Ok(ItemRow {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    repo: row.get(2)?,
                    title: row.get(3)?,
                    body: row.get(4)?,
                    item_type: row.get(5)?,
                    state: row.get(6)?,
                    reason: row.get(7)?,
                    author: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                    first_seen_at: row.get(11)?,
                    last_activity_at: row.get(12)?,
                    comment_count: row.get(13)?,
                    summary: row.get(14)?,
                    status_str: row.get(15)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        items.into_iter().map(|r| r.into_item()).collect()
    }

    pub fn archive_item(&self, id: &str) -> Result<bool, DbError> {
        let rows = self.conn.execute(
            "UPDATE items SET status = 'archived' WHERE id = ?1 AND status = 'active'",
            params![id],
        )?;
        Ok(rows > 0)
    }

    /// Return the set of all repos that have ever had items in the DB.
    /// Used to suppress notifications when first polling a newly-added repo.
    pub fn known_repos(&self) -> Result<std::collections::HashSet<String>, DbError> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT repo FROM items")?;
        let repos = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        Ok(repos)
    }

    pub fn set_summary(&self, id: &str, summary: &str) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE items SET summary = ?1 WHERE id = ?2",
            params![summary, id],
        )?;
        Ok(())
    }

    pub fn get_last_poll(&self) -> Result<Option<DateTime<Utc>>, DbError> {
        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'last_poll'",
                [],
                |row| row.get(0),
            )
            .ok();
        match result {
            Some(s) => {
                let dt = DateTime::parse_from_rfc3339(&s)?.with_timezone(&Utc);
                Ok(Some(dt))
            }
            None => Ok(None),
        }
    }

    pub fn set_last_poll(&self, time: DateTime<Utc>) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('last_poll', ?1)",
            params![time.to_rfc3339()],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn active_count(&self) -> Result<usize, DbError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM items WHERE status = 'active'",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

struct ItemRow {
    id: String,
    url: String,
    repo: String,
    title: String,
    body: Option<String>,
    item_type: String,
    state: String,
    reason: Option<String>,
    author: String,
    created_at: String,
    updated_at: String,
    first_seen_at: String,
    last_activity_at: Option<String>,
    comment_count: u32,
    summary: Option<String>,
    status_str: String,
}

impl ItemRow {
    fn into_item(self) -> Result<Item, DbError> {
        Ok(Item {
            id: self.id,
            url: self.url,
            repo: self.repo,
            title: self.title,
            body: self.body,
            item_type: ItemType::from_db_str(&self.item_type),
            state: self.state,
            reason: self.reason.unwrap_or_else(|| "unknown".to_string()),
            author: self.author,
            created_at: DateTime::parse_from_rfc3339(&self.created_at)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&self.updated_at)?.with_timezone(&Utc),
            first_seen_at: DateTime::parse_from_rfc3339(&self.first_seen_at)?.with_timezone(&Utc),
            last_activity_at: self
                .last_activity_at
                .map(|s| DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)))
                .transpose()?,
            comment_count: self.comment_count,
            summary: self.summary,
            status: ItemStatus::from_db_str(&self.status_str),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ItemStatus, ItemType};

    fn make_item(id: &str, title: &str) -> Item {
        let now = Utc::now();
        Item {
            id: id.to_string(),
            url: format!("https://github.com/org/repo/issues/{}", id),
            repo: "org/repo".to_string(),
            title: title.to_string(),
            body: Some("body text".to_string()),
            item_type: ItemType::Issue,
            state: "open".to_string(),
            reason: "assigned".to_string(),
            author: "alice".to_string(),
            created_at: now,
            updated_at: now,
            first_seen_at: now,
            last_activity_at: Some(now),
            comment_count: 0,
            summary: None,
            status: ItemStatus::Active,
        }
    }

    #[test]
    fn insert_and_fetch() {
        let db = Db::open_in_memory().unwrap();
        let item = make_item("node1", "Test issue");
        let (inserted, updated, _) = db.upsert_item(&item).unwrap();
        assert!(inserted);
        assert!(!updated);

        let items = db.get_items(ItemStatus::Active).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Test issue");
    }

    #[test]
    fn duplicate_insert_is_noop() {
        let db = Db::open_in_memory().unwrap();
        let item = make_item("node1", "Test issue");
        db.upsert_item(&item).unwrap();
        let (inserted, updated, _) = db.upsert_item(&item).unwrap();
        assert!(!inserted);
        assert!(!updated);
        assert_eq!(db.get_items(ItemStatus::Active).unwrap().len(), 1);
    }

    #[test]
    fn archive_item() {
        let db = Db::open_in_memory().unwrap();
        let item = make_item("node1", "Test issue");
        db.upsert_item(&item).unwrap();

        assert!(db.archive_item("node1").unwrap());
        assert_eq!(db.get_items(ItemStatus::Active).unwrap().len(), 0);
        assert_eq!(db.get_items(ItemStatus::Archived).unwrap().len(), 1);
    }

    #[test]
    fn update_on_changed_updated_at() {
        let db = Db::open_in_memory().unwrap();
        let mut item = make_item("node1", "Test issue");
        db.upsert_item(&item).unwrap();

        item.updated_at = item.updated_at + chrono::Duration::seconds(60);
        item.title = "Updated title".to_string();
        let (inserted, updated, _) = db.upsert_item(&item).unwrap();
        assert!(!inserted);
        assert!(updated);

        let items = db.get_items(ItemStatus::Active).unwrap();
        assert_eq!(items[0].title, "Updated title");
    }
}
