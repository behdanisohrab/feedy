// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

use crate::db::Database;
use crate::model::{Entry, Feed, Pane};
use crate::sync::SyncService;
use crate::validation::validate_feed_url;
use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::io;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct App {
    pub db: Database,
    pub sync: SyncService,
    feeds: Vec<Feed>,
    entries: Vec<Entry>,
    feed_idx: usize,
    entry_idx: usize,
    pane: Pane,
    show_unread_only: bool,
    show_starred_only: bool,
    include_hidden: bool,
    last_refresh: Option<Instant>,
    refresh_interval: Option<Duration>,
    status: String,
    input_mode: InputMode,
    screen: Screen,
    add_feed_input: String,
    command_input: String,
    preview_scroll: u16,
    g_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    AddFeed,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Help,
    About,
}

impl App {
    pub fn new(
        db: Database,
        sync: SyncService,
        refresh_interval_min: Option<u64>,
    ) -> anyhow::Result<Self> {
        let mut app = Self {
            db,
            sync,
            feeds: Vec::new(),
            entries: Vec::new(),
            feed_idx: 0,
            entry_idx: 0,
            pane: Pane::Feeds,
            show_unread_only: false,
            show_starred_only: false,
            include_hidden: false,
            last_refresh: None,
            refresh_interval: refresh_interval_min.map(|m| Duration::from_secs(m * 60)),
            status: "Ready".to_string(),
            input_mode: InputMode::Normal,
            screen: Screen::Main,
            add_feed_input: String::new(),
            command_input: String::new(),
            preview_scroll: 0,
            g_pending: false,
        };
        app.reload()?;
        Ok(app)
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let mut terminal = ratatui::init();
        let res = self.run_loop(&mut terminal);
        ratatui::restore();
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen)?;
        res
    }

    fn run_loop(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        loop {
            terminal.draw(|f| self.render(f))?;

            if self.input_mode == InputMode::Normal
                && self.screen == Screen::Main
                && let Some(interval) = self.refresh_interval
                && self
                    .last_refresh
                    .map(|x| x.elapsed() >= interval)
                    .unwrap_or(true)
            {
                let _ = self.refresh_all();
            }

            if event::poll(Duration::from_millis(120))? {
                let ev = event::read()?;
                if let Event::Key(key) = ev {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if self.handle_key(key.code)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode) -> anyhow::Result<bool> {
        match self.input_mode {
            InputMode::AddFeed => return self.handle_add_feed_input(code),
            InputMode::Command => return self.handle_command_input(code),
            InputMode::Normal => {}
        }

        if self.screen != Screen::Main {
            match code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.screen = Screen::Main;
                    self.status = "Returned to main".to_string();
                }
                KeyCode::Char(':') => {
                    self.input_mode = InputMode::Command;
                    self.command_input.clear();
                }
                _ => {}
            }
            return Ok(false);
        }

        match code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => self.screen = Screen::Help,
            KeyCode::Char(':') => {
                self.input_mode = InputMode::Command;
                self.command_input.clear();
            }
            KeyCode::Char('a') => {
                self.input_mode = InputMode::AddFeed;
                self.add_feed_input.clear();
                self.status = "Add feed URL and press Enter".to_string();
            }
            KeyCode::Char('D') => {
                self.delete_selected_feed()?;
            }
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.prev(),
            KeyCode::PageDown => self.preview_scroll = self.preview_scroll.saturating_add(10),
            KeyCode::PageUp => self.preview_scroll = self.preview_scroll.saturating_sub(10),
            KeyCode::Char('h') | KeyCode::Left => self.focus_prev_pane(),
            KeyCode::Char('l') | KeyCode::Right => self.focus_next_pane(),
            KeyCode::Char('g') => {
                if self.g_pending {
                    self.jump_top();
                    self.g_pending = false;
                } else {
                    self.g_pending = true;
                    self.status = "g pending. press g again for top".to_string();
                }
            }
            KeyCode::Char('G') => self.jump_bottom(),
            KeyCode::Char('r') => {
                self.refresh_current_feed()?;
                self.reload_entries()?;
            }
            KeyCode::Char('R') => {
                self.refresh_all()?;
                self.reload_entries()?;
            }
            KeyCode::Char('u') => {
                self.show_unread_only = !self.show_unread_only;
                self.reload_entries()?;
            }
            KeyCode::Char('f') => {
                self.show_starred_only = !self.show_starred_only;
                self.reload_entries()?;
            }
            KeyCode::Char('H') => {
                self.include_hidden = !self.include_hidden;
                self.reload_entries()?;
                self.status = if self.include_hidden {
                    "Hidden entries are visible".to_string()
                } else {
                    "Hidden entries are hidden".to_string()
                };
            }
            KeyCode::Char('m') => {
                if let Some(id) = self.current_entry_id() {
                    self.db.toggle_read(id)?;
                    self.reload_entries()?;
                }
            }
            KeyCode::Char('s') => {
                if let Some(id) = self.current_entry_id() {
                    self.db.toggle_star(id)?;
                    self.reload_entries()?;
                }
            }
            KeyCode::Char('x') => {
                if let Some(id) = self.current_entry_id() {
                    self.db.toggle_hidden(id)?;
                    self.reload_entries()?;
                    self.status =
                        "Toggled hidden state. Press H to show hidden entries".to_string();
                }
            }
            KeyCode::Char('o') => {
                if let Some(url) = self.current_entry().and_then(|e| e.url.clone()) {
                    match open_url(&url) {
                        Ok(()) => self.status = "Opened URL".to_string(),
                        Err(e) => self.status = format!("Failed to open URL: {e}"),
                    }
                }
            }
            _ => {
                self.g_pending = false;
            }
        }
        if !matches!(code, KeyCode::Char('g')) {
            self.g_pending = false;
        }
        Ok(false)
    }

    fn handle_add_feed_input(&mut self, code: KeyCode) -> anyhow::Result<bool> {
        match code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status = "Add feed canceled".to_string();
            }
            KeyCode::Enter => {
                let raw = self.add_feed_input.trim().to_string();
                match validate_feed_url(&raw) {
                    Ok(url) => {
                        let feed_id = self.db.upsert_feed(&url, None, &url)?;
                        match self.sync.refresh_feed(&self.db, feed_id, &url) {
                            Ok(n) => self.status = format!("Added feed and synced {n} entries"),
                            Err(e) => self.status = format!("Added feed but sync failed: {e:#}"),
                        }
                        self.reload()?;
                    }
                    Err(e) => {
                        self.status = format!("Invalid feed URL: {e}");
                    }
                }
                self.add_feed_input.clear();
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.add_feed_input.pop();
            }
            KeyCode::Char(c) => {
                if !c.is_control() {
                    self.add_feed_input.push(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_command_input(&mut self, code: KeyCode) -> anyhow::Result<bool> {
        match code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.command_input.clear();
                self.status = "Command canceled".to_string();
            }
            KeyCode::Enter => {
                let cmd = self.command_input.trim().to_ascii_lowercase();
                self.command_input.clear();
                self.input_mode = InputMode::Normal;
                return self.execute_command(&cmd);
            }
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Char(c) => {
                if !c.is_control() {
                    self.command_input.push(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn execute_command(&mut self, cmd: &str) -> anyhow::Result<bool> {
        match cmd {
            "q" | "quit" => return Ok(true),
            "help" => {
                self.screen = Screen::Help;
                self.status = "Help".to_string();
            }
            "about" => {
                self.screen = Screen::About;
                self.status = "About".to_string();
            }
            "main" | "close" => {
                self.screen = Screen::Main;
                self.status = "Main".to_string();
            }
            "hidden" => {
                self.include_hidden = !self.include_hidden;
                self.reload_entries()?;
                self.status = if self.include_hidden {
                    "Hidden entries are visible".to_string()
                } else {
                    "Hidden entries are hidden".to_string()
                };
            }
            "refresh" => {
                self.refresh_all()?;
                self.reload_entries()?;
            }
            "deletefeed" => {
                self.delete_selected_feed()?;
            }
            "unread" => {
                self.show_unread_only = !self.show_unread_only;
                self.reload_entries()?;
            }
            "starred" => {
                self.show_starred_only = !self.show_starred_only;
                self.reload_entries()?;
            }
            _ => {
                self.status = format!("Unknown command: {cmd}");
            }
        }
        Ok(false)
    }

    fn next(&mut self) {
        match self.pane {
            Pane::Feeds => {
                if !self.feeds.is_empty() {
                    self.feed_idx = (self.feed_idx + 1).min(self.feeds.len() - 1);
                    self.preview_scroll = 0;
                    let _ = self.reload_entries();
                }
            }
            Pane::Entries => {
                if !self.entries.is_empty() {
                    self.entry_idx = (self.entry_idx + 1).min(self.entries.len() - 1);
                    self.preview_scroll = 0;
                }
            }
            Pane::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_add(2);
            }
        }
    }

    fn prev(&mut self) {
        match self.pane {
            Pane::Feeds => {
                if self.feed_idx > 0 {
                    self.feed_idx -= 1;
                    self.preview_scroll = 0;
                    let _ = self.reload_entries();
                }
            }
            Pane::Entries => {
                if self.entry_idx > 0 {
                    self.entry_idx -= 1;
                    self.preview_scroll = 0;
                }
            }
            Pane::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_sub(2);
            }
        }
    }

    fn jump_top(&mut self) {
        match self.pane {
            Pane::Feeds => {
                self.feed_idx = 0;
                self.preview_scroll = 0;
                let _ = self.reload_entries();
            }
            Pane::Entries => {
                self.entry_idx = 0;
                self.preview_scroll = 0;
            }
            Pane::Preview => self.preview_scroll = 0,
        }
    }

    fn jump_bottom(&mut self) {
        match self.pane {
            Pane::Feeds => {
                if !self.feeds.is_empty() {
                    self.feed_idx = self.feeds.len() - 1;
                    self.preview_scroll = 0;
                    let _ = self.reload_entries();
                }
            }
            Pane::Entries => {
                if !self.entries.is_empty() {
                    self.entry_idx = self.entries.len() - 1;
                    self.preview_scroll = 0;
                }
            }
            Pane::Preview => self.preview_scroll = self.preview_scroll.saturating_add(50),
        }
    }

    fn focus_prev_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Feeds => Pane::Preview,
            Pane::Entries => Pane::Feeds,
            Pane::Preview => Pane::Entries,
        };
    }

    fn focus_next_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Feeds => Pane::Entries,
            Pane::Entries => Pane::Preview,
            Pane::Preview => Pane::Feeds,
        };
    }

    fn reload(&mut self) -> anyhow::Result<()> {
        self.feeds = self.db.all_feeds()?;
        if self.feed_idx >= self.feeds.len() {
            self.feed_idx = self.feeds.len().saturating_sub(1);
        }
        self.reload_entries()?;
        Ok(())
    }

    fn delete_selected_feed(&mut self) -> anyhow::Result<()> {
        if let Some(feed) = self.feeds.get(self.feed_idx).cloned() {
            let affected = self.db.delete_feed(&feed.id.to_string())?;
            if affected > 0 {
                self.status = format!("Deleted feed: {}", feed.title);
                self.feed_idx = self.feed_idx.saturating_sub(1);
                self.reload()?;
            } else {
                self.status = "No feed deleted".to_string();
            }
        } else {
            self.status = "No feed selected".to_string();
        }
        Ok(())
    }

    fn reload_entries(&mut self) -> anyhow::Result<()> {
        let feed_id = self.feeds.get(self.feed_idx).map(|f| f.id);
        self.entries = self.db.list_entries(
            feed_id,
            self.show_unread_only,
            self.show_starred_only,
            self.include_hidden,
        )?;
        if self.entry_idx >= self.entries.len() {
            self.entry_idx = self.entries.len().saturating_sub(1);
        }
        Ok(())
    }

    fn refresh_current_feed(&mut self) -> anyhow::Result<()> {
        if let Some(feed) = self.feeds.get(self.feed_idx).cloned() {
            match self.sync.refresh_feed(&self.db, feed.id, &feed.feed_url) {
                Ok(n) => self.status = format!("Refreshed {n} entries for {}", feed.title),
                Err(e) => self.status = format!("Refresh failed: {e:#}"),
            }
            self.last_refresh = Some(Instant::now());
            self.reload()?;
        }
        Ok(())
    }

    fn refresh_all(&mut self) -> anyhow::Result<()> {
        let feeds = self.db.all_feeds()?;
        let mut total = 0usize;
        let mut failed = 0usize;
        for f in feeds {
            match self.sync.refresh_feed(&self.db, f.id, &f.feed_url) {
                Ok(n) => total += n,
                Err(_) => failed += 1,
            }
        }
        self.last_refresh = Some(Instant::now());
        self.status = if failed == 0 {
            format!("Refreshed all feeds. {total} items updated")
        } else {
            format!("Refreshed with {failed} failed feeds. {total} items updated")
        };
        self.reload()?;
        Ok(())
    }

    fn current_entry_id(&self) -> Option<i64> {
        self.entries.get(self.entry_idx).map(|x| x.id)
    }

    fn current_entry(&self) -> Option<&Entry> {
        self.entries.get(self.entry_idx)
    }

    fn preview_body(entry: &Entry) -> String {
        let raw = entry
            .content
            .as_deref()
            .or(entry.summary.as_deref())
            .unwrap_or("(no content)");
        html2text::from_read(raw.as_bytes(), 120).unwrap_or_else(|_| raw.to_string())
    }

    fn render(&self, f: &mut Frame) {
        match self.screen {
            Screen::Main => self.render_main(f),
            Screen::Help => self.render_help(f),
            Screen::About => self.render_about(f),
        }
        if self.input_mode == InputMode::AddFeed {
            self.render_add_feed_modal(f);
        }
    }

    fn render_main(&self, f: &mut Frame) {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(f.area());

        let title = Paragraph::new(Line::from(vec![
            Span::styled("feedy", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" | RSS/Atom reader"),
        ]));
        f.render_widget(title, root[0]);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(35),
                Constraint::Percentage(40),
            ])
            .split(root[1]);

        let feed_items: Vec<ListItem> = self
            .feeds
            .iter()
            .map(|fd| {
                let checked = fd
                    .last_checked_at
                    .map(|d| d.format("%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".to_string());
                ListItem::new(format!("{} ({checked})", fd.title))
            })
            .collect();

        let entries_items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let unread = if e.is_read { " " } else { "U" };
                let starred = if e.is_starred { "S" } else { " " };
                let hidden = if e.is_hidden { "H" } else { " " };
                ListItem::new(format!("[{unread}{starred}{hidden}] {}", e.title))
            })
            .collect();

        let selected_style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);

        let mut feeds_state = ListState::default().with_selected(Some(self.feed_idx));
        let mut entries_state = ListState::default().with_selected(Some(self.entry_idx));

        let feed_block = Block::default()
            .title(if self.pane == Pane::Feeds {
                Line::styled("Feeds", Style::default().add_modifier(Modifier::BOLD))
            } else {
                Line::from("Feeds")
            })
            .borders(Borders::ALL);

        let entries_block = Block::default()
            .title(if self.pane == Pane::Entries {
                Line::styled("Entries", Style::default().add_modifier(Modifier::BOLD))
            } else {
                Line::from("Entries")
            })
            .borders(Borders::ALL);

        let preview_block = Block::default()
            .title(if self.pane == Pane::Preview {
                Line::styled("Preview", Style::default().add_modifier(Modifier::BOLD))
            } else {
                Line::from("Preview")
            })
            .borders(Borders::ALL);

        f.render_stateful_widget(
            List::new(feed_items)
                .block(feed_block)
                .highlight_style(selected_style),
            cols[0],
            &mut feeds_state,
        );
        f.render_stateful_widget(
            List::new(entries_items)
                .block(entries_block)
                .highlight_style(selected_style),
            cols[1],
            &mut entries_state,
        );

        let preview_lines = if let Some(e) = self.current_entry() {
            let mut lines = vec![Line::from(Span::styled(
                e.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ))];
            if let Some(a) = &e.author {
                lines.push(Line::from(format!("by {a}")));
            }
            if let Some(d) = e.published_at {
                lines.push(Line::from(d.to_rfc3339()));
            }
            lines.push(Line::from(""));
            lines.extend(Text::from(Self::preview_body(e)).lines);
            lines
        } else {
            vec![Line::from("No entry selected")]
        };

        f.render_widget(
            Paragraph::new(preview_lines)
                .block(preview_block)
                .scroll((self.preview_scroll, 0))
                .wrap(Wrap { trim: true }),
            cols[2],
        );

        let footer = match self.input_mode {
            InputMode::Normal => {
                format!("{} | :help for cheat sheet | :about | :q", self.status)
            }
            InputMode::Command => format!(":{}", self.command_input),
            InputMode::AddFeed => self.status.clone(),
        };

        f.render_widget(
            Paragraph::new(footer).block(Block::default().borders(Borders::TOP)),
            root[2],
        );
    }

    fn render_help(&self, f: &mut Frame) {
        let lines = vec![
            Line::from(Span::styled(
                "Help / Cheat Sheet",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Global"),
            Line::from("q             quit app"),
            Line::from("?             open help page"),
            Line::from(":             open command line"),
            Line::from("Esc           close help/about or cancel input"),
            Line::from(""),
            Line::from("Main view"),
            Line::from("j or Down     move down in focused pane"),
            Line::from("k or Up       move up in focused pane"),
            Line::from("h or Left     focus previous pane"),
            Line::from("l or Right    focus next pane"),
            Line::from("gg            jump to top"),
            Line::from("G             jump to bottom"),
            Line::from("PageDown      scroll preview down"),
            Line::from("PageUp        scroll preview up"),
            Line::from(""),
            Line::from("Feed and sync"),
            Line::from("a             add feed URL in modal"),
            Line::from("D             delete selected feed"),
            Line::from("r             refresh current feed"),
            Line::from("R             refresh all feeds"),
            Line::from("o             open selected entry URL"),
            Line::from(""),
            Line::from("Entry state and filters"),
            Line::from("m             toggle read"),
            Line::from("s             toggle starred"),
            Line::from("x             toggle hidden"),
            Line::from("H             toggle show hidden entries"),
            Line::from("u             toggle unread-only filter"),
            Line::from("f             toggle starred-only filter"),
            Line::from(""),
            Line::from("Hidden recovery"),
            Line::from("H -> select hidden entry -> x"),
            Line::from(""),
            Line::from("Command mode"),
            Line::from(":help         open help page"),
            Line::from(":about        open about page"),
            Line::from(":main         return to main page"),
            Line::from(":refresh      refresh all feeds"),
            Line::from(":deletefeed   delete selected feed"),
            Line::from(":hidden       toggle show hidden entries"),
            Line::from(":unread       toggle unread-only filter"),
            Line::from(":starred      toggle starred-only filter"),
            Line::from(":q or :quit   quit app"),
        ];

        let block = Block::default().title("Help").borders(Borders::ALL);
        f.render_widget(
            Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
            f.area(),
        );

        if self.input_mode == InputMode::Command {
            let input = Paragraph::new(format!(":{}", self.command_input))
                .block(Block::default().borders(Borders::TOP));
            let area = bottom_bar_area(f.area());
            f.render_widget(Clear, area);
            f.render_widget(input, area);
        }
    }

    fn render_about(&self, f: &mut Frame) {
        let lines = vec![
            Line::from(Span::styled(
                "About feedy",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("feedy is a local-first terminal feed reader."),
            Line::from("It supports RSS and Atom, SQLite persistence, read/star/hide state,"),
            Line::from("OPML import/export, and keyboard-first workflows."),
            Line::from(""),
            Line::from("Use :help to open the cheat sheet."),
            Line::from("Esc or q returns to main."),
        ];

        f.render_widget(
            Paragraph::new(lines)
                .block(Block::default().title("About").borders(Borders::ALL))
                .wrap(Wrap { trim: true }),
            f.area(),
        );

        if self.input_mode == InputMode::Command {
            let input = Paragraph::new(format!(":{}", self.command_input))
                .block(Block::default().borders(Borders::TOP));
            let area = bottom_bar_area(f.area());
            f.render_widget(Clear, area);
            f.render_widget(input, area);
        }
    }

    fn render_add_feed_modal(&self, f: &mut Frame) {
        let area = centered_rect(70, 20, f.area());
        let input_block = Block::default().title("Add Feed URL").borders(Borders::ALL);
        let text = vec![
            Line::from("Enter feed URL and press Enter"),
            Line::from("Esc to cancel"),
            Line::from(""),
            Line::from(self.add_feed_input.clone()),
        ];

        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(text)
                .block(input_block)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: true }),
            area,
        );
    }
}

fn open_url(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn xdg-open")?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    {
        open::that(url).context("failed to open URL")?;
        Ok(())
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn bottom_bar_area(r: Rect) -> Rect {
    Rect {
        x: r.x,
        y: r.y + r.height.saturating_sub(2),
        width: r.width,
        height: 2,
    }
}

pub fn run_tui(
    db: Database,
    sync: SyncService,
    refresh_interval_min: Option<u64>,
) -> anyhow::Result<()> {
    let app = App::new(db, sync, refresh_interval_min).context("failed to initialize app")?;
    app.run()
}
