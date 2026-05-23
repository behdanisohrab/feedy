// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

use crate::model::{Entry, Feed};
use anyhow::Context;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Database {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct NewEntry {
    pub feed_id: i64,
    pub key: String,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub summary: Option<String>,
    pub content: Option<String>,
}

impl Database {
    const SCHEMA_VERSION: i64 = 1;

    pub fn default_db_path() -> anyhow::Result<PathBuf> {
        let base = dirs::data_local_dir().context("missing local data directory")?;
        let dir = base.join("feedy");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("feedy.db"))
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        self.conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let user_version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if user_version > Self::SCHEMA_VERSION {
            anyhow::bail!(
                "database schema version {} is newer than supported {}",
                user_version,
                Self::SCHEMA_VERSION
            );
        }
        if user_version < 1 {
            self.conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS feeds (
                  id INTEGER PRIMARY KEY,
                  title TEXT NOT NULL,
                  site_url TEXT,
                  feed_url TEXT NOT NULL UNIQUE,
                  etag TEXT,
                  last_modified TEXT,
                  last_checked_at TEXT,
                  is_active INTEGER NOT NULL DEFAULT 1,
                  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS entries (
                  id INTEGER PRIMARY KEY,
                  feed_id INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
                  guid_or_link_key TEXT NOT NULL,
                  title TEXT NOT NULL,
                  author TEXT,
                  url TEXT,
                  published_at TEXT,
                  updated_at TEXT,
                  summary TEXT,
                  content TEXT,
                  fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                  is_hidden INTEGER NOT NULL DEFAULT 0,
                  UNIQUE(feed_id, guid_or_link_key)
                );

                CREATE TABLE IF NOT EXISTS entry_state (
                  entry_id INTEGER PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
                  is_read INTEGER NOT NULL DEFAULT 0,
                  is_starred INTEGER NOT NULL DEFAULT 0,
                  read_at TEXT,
                  starred_at TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_entries_feed_id ON entries(feed_id);
                CREATE INDEX IF NOT EXISTS idx_entries_pub ON entries(published_at DESC);
                CREATE INDEX IF NOT EXISTS idx_entry_state_read ON entry_state(is_read);
                CREATE INDEX IF NOT EXISTS idx_entry_state_starred ON entry_state(is_starred);
                "#,
            )?;
            self.conn
                .execute_batch(&format!("PRAGMA user_version = {};", Self::SCHEMA_VERSION))?;
        }
        Ok(())
    }

    pub fn all_feeds(&self) -> anyhow::Result<Vec<Feed>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, site_url, feed_url, last_checked_at FROM feeds WHERE is_active = 1 ORDER BY title",
        )?;
        let rows = stmt.query_map([], |row| {
            let ts: Option<String> = row.get(4)?;
            Ok(Feed {
                id: row.get(0)?,
                title: row.get(1)?,
                site_url: row.get(2)?,
                feed_url: row.get(3)?,
                last_checked_at: ts.and_then(|v| {
                    DateTime::parse_from_rfc3339(&v)
                        .ok()
                        .map(|x| x.with_timezone(&Utc))
                }),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn upsert_feed(
        &self,
        title: &str,
        site_url: Option<&str>,
        feed_url: &str,
    ) -> anyhow::Result<i64> {
        self.conn.execute(
            r#"
            INSERT INTO feeds (title, site_url, feed_url, updated_at)
            VALUES (?, ?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(feed_url) DO UPDATE SET
              title=excluded.title,
              site_url=excluded.site_url,
              is_active=1,
              updated_at=CURRENT_TIMESTAMP
            "#,
            params![title, site_url, feed_url],
        )?;

        let id: i64 =
            self.conn
                .query_row("SELECT id FROM feeds WHERE feed_url = ?", [feed_url], |r| {
                    r.get(0)
                })?;
        Ok(id)
    }

    pub fn remove_feed(&self, key: &str) -> anyhow::Result<usize> {
        let affected = if let Ok(id) = key.parse::<i64>() {
            self.conn
                .execute("UPDATE feeds SET is_active = 0 WHERE id = ?", [id])?
        } else {
            self.conn
                .execute("UPDATE feeds SET is_active = 0 WHERE feed_url = ?", [key])?
        };
        Ok(affected)
    }

    pub fn delete_feed(&self, key: &str) -> anyhow::Result<usize> {
        let affected = if let Ok(id) = key.parse::<i64>() {
            self.conn.execute("DELETE FROM feeds WHERE id = ?", [id])?
        } else {
            self.conn
                .execute("DELETE FROM feeds WHERE feed_url = ?", [key])?
        };
        Ok(affected)
    }

    pub fn set_feed_headers(
        &self,
        feed_id: i64,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE feeds SET etag = ?, last_modified = ?, last_checked_at = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            params![etag, last_modified, Utc::now().to_rfc3339(), feed_id],
        )?;
        Ok(())
    }

    pub fn feed_headers(&self, feed_id: i64) -> anyhow::Result<(Option<String>, Option<String>)> {
        let x = self.conn.query_row(
            "SELECT etag, last_modified FROM feeds WHERE id = ?",
            [feed_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(x)
    }

    pub fn upsert_entry(&self, e: &NewEntry) -> anyhow::Result<i64> {
        self.conn.execute(
            r#"
            INSERT INTO entries (feed_id, guid_or_link_key, title, author, url, published_at, updated_at, summary, content)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(feed_id, guid_or_link_key) DO UPDATE SET
              title=excluded.title,
              author=excluded.author,
              url=excluded.url,
              published_at=COALESCE(excluded.published_at, entries.published_at),
              updated_at=COALESCE(excluded.updated_at, entries.updated_at),
              summary=COALESCE(excluded.summary, entries.summary),
              content=COALESCE(excluded.content, entries.content)
            "#,
            params![
                e.feed_id,
                e.key,
                e.title,
                e.author,
                e.url,
                e.published_at.map(|x| x.to_rfc3339()),
                e.updated_at.map(|x| x.to_rfc3339()),
                e.summary,
                e.content
            ],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM entries WHERE feed_id = ? AND guid_or_link_key = ?",
            params![e.feed_id, e.key],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO entry_state (entry_id) VALUES (?)",
            [id],
        )?;
        Ok(id)
    }

    pub fn list_entries(
        &self,
        feed_id: Option<i64>,
        unread_only: bool,
        starred_only: bool,
        include_hidden: bool,
    ) -> anyhow::Result<Vec<Entry>> {
        let base = r#"
            SELECT e.id, e.feed_id, e.title, e.author, e.url, e.published_at, e.summary, e.content,
                   COALESCE(s.is_read,0), COALESCE(s.is_starred,0), e.is_hidden
            FROM entries e
            LEFT JOIN entry_state s ON s.entry_id = e.id
            WHERE (?1 IS NULL OR e.feed_id = ?1)
              AND (?2 = 0 OR COALESCE(s.is_read,0) = 0)
              AND (?3 = 0 OR COALESCE(s.is_starred,0) = 1)
              AND (?4 = 1 OR e.is_hidden = 0)
            ORDER BY COALESCE(e.published_at, e.updated_at, e.fetched_at) DESC
        "#;
        let mut stmt = self.conn.prepare(base)?;
        let rows = stmt.query_map(
            params![
                feed_id,
                unread_only as i32,
                starred_only as i32,
                include_hidden as i32
            ],
            |row| {
                let published: Option<String> = row.get(5)?;
                Ok(Entry {
                    id: row.get(0)?,
                    title: row.get(2)?,
                    author: row.get(3)?,
                    url: row.get(4)?,
                    published_at: published.and_then(|v| {
                        DateTime::parse_from_rfc3339(&v)
                            .ok()
                            .map(|x| x.with_timezone(&Utc))
                    }),
                    summary: row.get(6)?,
                    content: row.get(7)?,
                    is_read: row.get::<_, i64>(8)? != 0,
                    is_starred: row.get::<_, i64>(9)? != 0,
                    is_hidden: row.get::<_, i64>(10)? != 0,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn toggle_read(&self, entry_id: i64) -> anyhow::Result<()> {
        let cur: Option<i64> = self
            .conn
            .query_row(
                "SELECT is_read FROM entry_state WHERE entry_id = ?",
                [entry_id],
                |r| r.get(0),
            )
            .optional()?;
        let new_val = cur.unwrap_or(0) == 0;
        self.conn.execute(
            "INSERT INTO entry_state(entry_id,is_read,read_at) VALUES(?,?,?)
             ON CONFLICT(entry_id) DO UPDATE SET is_read=excluded.is_read, read_at=excluded.read_at",
            params![entry_id, new_val as i32, if new_val { Some(Utc::now().to_rfc3339()) } else { None }],
        )?;
        Ok(())
    }

    pub fn toggle_star(&self, entry_id: i64) -> anyhow::Result<()> {
        let cur: Option<i64> = self
            .conn
            .query_row(
                "SELECT is_starred FROM entry_state WHERE entry_id = ?",
                [entry_id],
                |r| r.get(0),
            )
            .optional()?;
        let new_val = cur.unwrap_or(0) == 0;
        self.conn.execute(
            "INSERT INTO entry_state(entry_id,is_starred,starred_at) VALUES(?,?,?)
             ON CONFLICT(entry_id) DO UPDATE SET is_starred=excluded.is_starred, starred_at=excluded.starred_at",
            params![entry_id, new_val as i32, if new_val { Some(Utc::now().to_rfc3339()) } else { None }],
        )?;
        Ok(())
    }

    pub fn toggle_hidden(&self, entry_id: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE entries SET is_hidden = CASE WHEN is_hidden = 0 THEN 1 ELSE 0 END WHERE id = ?",
            [entry_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_db() -> Database {
        Database::open(Path::new(":memory:")).expect("open db")
    }

    #[test]
    fn upsert_feed_reuses_existing_row() {
        let db = temp_db();
        let id1 = db
            .upsert_feed(
                "Example",
                Some("https://example.com"),
                "https://example.com/feed.xml",
            )
            .expect("insert");
        let id2 = db
            .upsert_feed("Example2", None, "https://example.com/feed.xml")
            .expect("update");
        assert_eq!(id1, id2);
    }

    #[test]
    fn dedup_entry_by_feed_and_key() {
        let db = temp_db();
        let feed_id = db
            .upsert_feed("Example", None, "https://example.com/feed.xml")
            .expect("feed");
        let entry = NewEntry {
            feed_id,
            key: "entry-1".to_string(),
            title: "Title A".to_string(),
            author: None,
            url: Some("https://example.com/posts/1".to_string()),
            published_at: None,
            updated_at: None,
            summary: Some("Summary A".to_string()),
            content: None,
        };
        let id1 = db.upsert_entry(&entry).expect("entry insert");
        let mut updated = entry.clone();
        updated.title = "Title B".to_string();
        let id2 = db.upsert_entry(&updated).expect("entry update");
        assert_eq!(id1, id2);
        let entries = db
            .list_entries(Some(feed_id), false, false, true)
            .expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Title B");
    }

    #[test]
    fn toggle_states_work() {
        let db = temp_db();
        let feed_id = db
            .upsert_feed("Example", None, "https://example.com/feed.xml")
            .expect("feed");
        let entry = NewEntry {
            feed_id,
            key: "entry-2".to_string(),
            title: "Title".to_string(),
            author: None,
            url: None,
            published_at: None,
            updated_at: None,
            summary: None,
            content: None,
        };
        let id = db.upsert_entry(&entry).expect("entry");
        db.toggle_read(id).expect("toggle read");
        db.toggle_star(id).expect("toggle star");
        db.toggle_hidden(id).expect("toggle hidden");
        let entries = db
            .list_entries(Some(feed_id), false, false, true)
            .expect("list");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_read);
        assert!(entries[0].is_starred);
        assert!(entries[0].is_hidden);
    }
}
