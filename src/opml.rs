// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

use crate::db::Database;
use crate::validation::validate_feed_url;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
struct Opml {
    #[serde(rename = "body")]
    body: Body,
}

#[derive(Debug, Serialize, Deserialize)]
struct Body {
    #[serde(rename = "outline", default)]
    outline: Vec<Outline>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Outline {
    #[serde(rename = "@text")]
    text: Option<String>,
    #[serde(rename = "@title")]
    title: Option<String>,
    #[serde(rename = "@xmlUrl")]
    xml_url: Option<String>,
    #[serde(rename = "@htmlUrl")]
    html_url: Option<String>,
}

pub fn import_opml(path: &Path, db: &Database) -> anyhow::Result<usize> {
    let xml =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    let doc: Opml = quick_xml::de::from_str(&xml).context("invalid OPML")?;
    let mut count = 0usize;
    for o in doc.body.outline {
        if let Some(feed_url) = o.xml_url {
            let feed_url = match validate_feed_url(&feed_url) {
                Ok(url) => url,
                Err(_) => continue,
            };
            let title = o.title.or(o.text).unwrap_or_else(|| feed_url.clone());
            db.upsert_feed(&title, o.html_url.as_deref(), &feed_url)?;
            count += 1;
        }
    }
    Ok(count)
}

pub fn export_opml(path: &Path, db: &Database) -> anyhow::Result<()> {
    let feeds = db.all_feeds()?;
    let doc = Opml {
        body: Body {
            outline: feeds
                .into_iter()
                .map(|f| Outline {
                    text: Some(f.title.clone()),
                    title: Some(f.title),
                    xml_url: Some(f.feed_url),
                    html_url: f.site_url,
                })
                .collect(),
        },
    };

    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n");
    let body = quick_xml::se::to_string(&doc)?;
    xml.push_str(&body.replace("<Opml>", "").replace("</Opml>", ""));
    xml.push_str("\n</opml>\n");
    fs::write(path, xml)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::NamedTempFile;

    #[test]
    fn import_export_roundtrip() {
        let db_file = NamedTempFile::new().expect("db temp");
        let db = Database::open(db_file.path()).expect("open db");

        let in_file = NamedTempFile::new().expect("input temp");
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <body>
    <outline text="Example" xmlUrl="https://example.com/feed.xml" htmlUrl="https://example.com" />
  </body>
</opml>"#;
        fs::write(in_file.path(), input).expect("write input");
        let imported = import_opml(in_file.path(), &db).expect("import");
        assert_eq!(imported, 1);

        let out_file = NamedTempFile::new().expect("output temp");
        export_opml(out_file.path(), &db).expect("export");
        let out = fs::read_to_string(out_file.path()).expect("read output");
        assert!(out.contains("https://example.com/feed.xml"));
    }
}
