# feedy

feedy is a terminal feed reader built with Rust, ratatui, and SQLite.

Author: Sohrab Behdani
License: GNU AGPL v3.0 or later

## Install

```bash
cargo install --path .
```

## Run

```bash
feedy
```

## CLI commands

```bash
feedy add <feed_url>
feedy remove <feed_url_or_feed_id>
feedy delete <feed_url_or_feed_id>
feedy refresh --all
feedy refresh --feed <id>
feedy import-opml <path>
feedy export-opml <path>
feedy export-news <path> [--feed <id>] [--include-hidden]
feedy doctor
```

`remove` disables a feed. `delete` removes it and its entries from the database.

`export-news` writes entries to a Markdown file.

## TUI shortcuts

`q` quit
`?` help page
`:` command mode
`Esc` close help/about or cancel input

`j` `k` move
`h` `l` change pane
`gg` top
`G` bottom
`PageUp` `PageDown` preview scroll

`a` add feed
`D` delete selected feed
`r` refresh selected feed
`R` refresh all feeds
`o` open selected entry link

`m` toggle read
`s` toggle star
`x` toggle hidden
`H` show or hide hidden entries
`u` unread-only filter
`f` starred-only filter

To restore hidden entries: press `H`, select a hidden entry, press `x`.

## Command mode

`:help`
`:about`
`:main`
`:refresh`
`:hidden`
`:unread`
`:starred`
`:deletefeed`
`:q` or `:quit`

## Data path

Default DB path:

`~/.local/share/feedy/feedy.db`
