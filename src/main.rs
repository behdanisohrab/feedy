// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

mod db;
mod export;
mod model;
mod opml;
mod sync;
mod tui;
mod validation;

use anyhow::Context;
use clap::{Parser, Subcommand};
use db::Database;
use export::export_news_markdown;
use std::path::PathBuf;
use sync::SyncService;
use validation::validate_feed_url;

#[derive(Parser)]
#[command(name = "feedy")]
#[command(about = "Terminal feed reader", long_about = None)]
struct Cli {
    #[arg(long)]
    db_path: Option<PathBuf>,

    #[arg(long, help = "Auto refresh interval in minutes")]
    refresh_interval: Option<u64>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Add {
        feed_url: String,
    },
    Remove {
        feed: String,
    },
    Delete {
        feed: String,
    },
    Refresh {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        feed: Option<i64>,
    },
    ImportOpml {
        path: PathBuf,
    },
    ExportOpml {
        path: PathBuf,
    },
    ExportNews {
        path: PathBuf,
        #[arg(long)]
        feed: Option<i64>,
        #[arg(long)]
        include_hidden: bool,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db_path.unwrap_or(Database::default_db_path()?);
    let db = Database::open(&db_path)
        .with_context(|| format!("failed opening db at {}", db_path.display()))?;

    match cli.command {
        None => {
            let sync = SyncService::new()?;
            tui::run_tui(db, sync, cli.refresh_interval)?;
        }
        Some(Commands::Add { feed_url }) => {
            let sync = SyncService::new()?;
            let validated = validate_feed_url(&feed_url)?;
            let title = validated.clone();
            let feed_id = db.upsert_feed(&title, None, &validated)?;
            let updated = sync.refresh_feed(&db, feed_id, &validated)?;
            println!("Added feed and synced {updated} entries");
        }
        Some(Commands::Remove { feed }) => {
            let affected = db.remove_feed(&feed)?;
            if affected == 0 {
                println!("No matching feed found");
            } else {
                println!("Feed removed");
            }
        }
        Some(Commands::Delete { feed }) => {
            let affected = db.delete_feed(&feed)?;
            if affected == 0 {
                println!("No matching feed found");
            } else {
                println!("Feed deleted permanently");
            }
        }
        Some(Commands::Refresh { all, feed }) => {
            let sync = SyncService::new()?;
            if all {
                let feeds = db.all_feeds()?;
                let mut count = 0usize;
                for f in feeds {
                    match sync.refresh_feed(&db, f.id, &f.feed_url) {
                        Ok(n) => count += n,
                        Err(e) => eprintln!("refresh failed for {}: {e:#}", f.feed_url),
                    }
                }
                println!("Refreshed all feeds; {count} entries updated");
            } else if let Some(id) = feed {
                let feeds = db.all_feeds()?;
                if let Some(f) = feeds.into_iter().find(|x| x.id == id) {
                    let n = sync.refresh_feed(&db, f.id, &f.feed_url)?;
                    println!("Refreshed {} entries", n);
                } else {
                    eprintln!("Feed id {id} not found");
                }
            } else {
                eprintln!("Use --all or --feed <id>");
            }
        }
        Some(Commands::ImportOpml { path }) => {
            let n = opml::import_opml(&path, &db)?;
            println!("Imported {n} feeds");
        }
        Some(Commands::ExportOpml { path }) => {
            opml::export_opml(&path, &db)?;
            println!("Exported feeds to {}", path.display());
        }
        Some(Commands::ExportNews {
            path,
            feed,
            include_hidden,
        }) => {
            let count = export_news_markdown(&db, &path, feed, include_hidden)?;
            println!("Exported {count} entries to {}", path.display());
        }
        Some(Commands::Doctor) => {
            let feeds = db.all_feeds()?;
            println!("DB path: {}", db_path.display());
            println!("Active feeds: {}", feeds.len());
            let sync = SyncService::new()?;
            if let Some(f) = feeds.first() {
                match sync.refresh_feed(&db, f.id, &f.feed_url) {
                    Ok(_) => println!("Network/parser check: ok"),
                    Err(e) => println!("Network/parser check failed: {e:#}"),
                }
            } else {
                println!("No feeds configured; network check skipped");
            }
        }
    }

    Ok(())
}
