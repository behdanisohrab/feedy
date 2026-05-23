// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub site_url: Option<String>,
    pub feed_url: String,
    pub last_checked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Feeds,
    Entries,
    Preview,
}
