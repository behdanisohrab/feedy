// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

use crate::db::{Database, NewEntry};
use anyhow::Context;
use feed_rs::model::Text;
use reqwest::blocking::Client;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use std::time::Duration;

pub struct SyncService {
    client: Client,
}

impl SyncService {
    pub fn new() -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("feedy/0.1")
            .build()?;
        Ok(Self { client })
    }

    pub fn refresh_feed(
        &self,
        db: &Database,
        feed_id: i64,
        feed_url: &str,
    ) -> anyhow::Result<usize> {
        let (etag, last_modified) = db.feed_headers(feed_id)?;
        let mut req = self.client.get(feed_url);
        if let Some(v) = etag.as_deref() {
            req = req.header(IF_NONE_MATCH, v);
        }
        if let Some(v) = last_modified.as_deref() {
            req = req.header(IF_MODIFIED_SINCE, v);
        }
        let resp = req
            .send()
            .with_context(|| format!("fetch failed: {feed_url}"))?;

        if resp.status().as_u16() == 304 {
            db.set_feed_headers(feed_id, etag.as_deref(), last_modified.as_deref())?;
            return Ok(0);
        }
        let new_etag = resp
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|x| x.to_owned());
        let new_last_mod = resp
            .headers()
            .get(LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|x| x.to_owned());

        let body = resp.bytes()?;
        let feed = feed_rs::parser::parse(&body[..]).context("failed to parse feed")?;
        let feed_title = get_text(feed.title.as_ref()).unwrap_or_else(|| feed_url.to_string());
        let site_url = feed.links.first().map(|l| l.href.as_str());
        db.upsert_feed(&feed_title, site_url, feed_url)?;

        let mut count = 0usize;
        for item in feed.entries {
            let key = item.id.trim().to_string();
            let fallback_link = item.links.first().map(|l| l.href.clone());
            let dedup_key = if !key.is_empty() {
                key
            } else {
                fallback_link
                    .clone()
                    .unwrap_or_else(|| format!("item-{}", count))
            };
            let title = get_text(item.title.as_ref()).unwrap_or_else(|| "(untitled)".to_string());
            let author = item.authors.first().map(|a| a.name.clone());
            let url = fallback_link;
            let published_at = item.published;
            let updated_at = item.updated;
            let summary = get_text(item.summary.as_ref());
            let content = item.content.and_then(|c| c.body);

            let ne = NewEntry {
                feed_id,
                key: dedup_key,
                title,
                author,
                url,
                published_at,
                updated_at,
                summary,
                content,
            };
            db.upsert_entry(&ne)?;
            count += 1;
        }

        db.set_feed_headers(feed_id, new_etag.as_deref(), new_last_mod.as_deref())?;
        Ok(count)
    }
}

fn get_text(t: Option<&Text>) -> Option<String> {
    t.map(|x| x.content.clone())
        .filter(|s| !s.trim().is_empty())
}
