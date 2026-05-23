// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Sohrab Behdani

use anyhow::Context;
use url::Url;

pub fn validate_feed_url(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("feed URL cannot be empty");
    }

    let url = Url::parse(trimmed).with_context(|| format!("invalid URL: {trimmed}"))?;
    match url.scheme() {
        "http" | "https" => {}
        _ => anyhow::bail!("feed URL must use http or https"),
    }

    if url.host_str().is_none() {
        anyhow::bail!("feed URL must include a host");
    }

    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_feed_url;

    #[test]
    fn accepts_http_and_https() {
        assert!(validate_feed_url("https://example.com/feed.xml").is_ok());
        assert!(validate_feed_url("http://example.com/feed.xml").is_ok());
    }

    #[test]
    fn rejects_non_http_scheme() {
        let err = validate_feed_url("ftp://example.com/feed.xml").expect_err("must fail");
        assert!(err.to_string().contains("http or https"));
    }
}
