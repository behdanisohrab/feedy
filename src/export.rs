// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

use crate::db::Database;
use anyhow::Context;
use std::fs;
use std::path::Path;

pub fn export_news_markdown(
    db: &Database,
    out_path: &Path,
    feed_id: Option<i64>,
    include_hidden: bool,
) -> anyhow::Result<usize> {
    let entries = db.list_entries(feed_id, false, false, include_hidden)?;
    let mut out = String::new();
    out.push_str("# feedy export\n\n");

    for entry in &entries {
        out.push_str("## ");
        out.push_str(&entry.title);
        out.push('\n');
        if let Some(url) = &entry.url {
            out.push_str("Link: ");
            out.push_str(url);
            out.push('\n');
        }
        if let Some(ts) = entry.published_at {
            out.push_str("Published: ");
            out.push_str(&ts.to_rfc3339());
            out.push('\n');
        }
        out.push_str("State: ");
        out.push_str(if entry.is_read { "read" } else { "unread" });
        out.push_str(", ");
        out.push_str(if entry.is_starred {
            "starred"
        } else {
            "not-starred"
        });
        out.push_str(", ");
        out.push_str(if entry.is_hidden { "hidden" } else { "visible" });
        out.push_str("\n\n");

        let body = entry
            .content
            .as_deref()
            .or(entry.summary.as_deref())
            .unwrap_or("(no content)");
        out.push_str(body);
        out.push_str("\n\n---\n\n");
    }

    fs::write(out_path, out).with_context(|| format!("failed writing {}", out_path.display()))?;
    Ok(entries.len())
}
