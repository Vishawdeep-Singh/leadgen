mod browser;
mod parser;

use std::num::NonZeroUsize;
use std::time::Duration;

use anyhow::{Context, Result};
use chromiumoxide::Browser;
use futures::stream::{self, StreamExt};
use tokio::time::timeout;

use crate::business::{Business, dedupe};

/// Settings for a scraping run.
#[derive(Debug, Clone)]
pub struct Config {
    /// Searches scraped at the same time.
    pub concurrency: NonZeroUsize,
    /// Place pages opened at the same time within one search.
    pub place_concurrency: NonZeroUsize,
    /// How many times to scroll the results list; more scrolls find more places.
    pub max_scrolls: usize,
    /// A single page (search results or place) that takes longer than this is abandoned.
    /// Only that page is lost; the rest of its search is kept.
    pub page_timeout: Duration,
    /// Show the Chrome window instead of running headless.
    pub show_browser: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            concurrency: NonZeroUsize::new(2).expect("2 is non-zero"),
            place_concurrency: NonZeroUsize::new(3).expect("3 is non-zero"),
            max_scrolls: 5,
            page_timeout: Duration::from_secs(30),
            show_browser: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchJob {
    pub query: String,
}
impl SearchJob {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
        }
    }
    pub fn url(&self) -> String {
        format!(
            "https://www.google.com/maps/search/{}",
            self.query.replace(' ', "+")
        )
    }
}

pub async fn run(jobs: Vec<SearchJob>, config: &Config) -> Result<Vec<Business>> {
    let (mut browser, events) = browser::launch(config.show_browser).await?;

    let results: Vec<Vec<Business>> = stream::iter(jobs)
        .map(|job| scrape_job(&browser, job, config))
        .buffer_unordered(config.concurrency.get())
        .collect()
        .await;

    // Leads are already scraped; a failed shutdown shouldn't lose them.
    browser.close().await.ok();
    events.await.ok();

    Ok(dedupe(results.into_iter().flatten().collect()))
}

/// Scrapes one search. On failure, logs and returns nothing so other searches continue.
async fn scrape_job(browser: &Browser, job: SearchJob, config: &Config) -> Vec<Business> {
    let url = job.url();
    eprintln!("\n📍 Scraping: {url}");

    match scrape(browser, &url, config).await {
        Ok(businesses) => {
            eprintln!("✅ {} prospects from {url}", businesses.len());
            businesses
        }
        Err(e) => {
            eprintln!("❌ Failed scraping {url}: {e}");
            Vec::new()
        }
    }
}

async fn scrape(browser: &Browser, search_url: &str, config: &Config) -> Result<Vec<Business>> {
    let links = within(
        config.page_timeout,
        browser::search_place_links(browser, search_url, config.max_scrolls),
    )
    .await?;
    println!("\n🔍 Found {} places, scraping details...", links.len());

    let mut places = stream::iter(links)
        .map(|link| async move {
            let result = within(config.page_timeout, scrape_place(browser, &link)).await;
            (link, result)
        })
        .buffer_unordered(config.place_concurrency.get());

    let mut businesses = Vec::new();

    while let Some((link, result)) = places.next().await {
        match result {
            Ok(b) => {
                businesses.push(b);
                print!("\r⏳ Scraped {} prospect", businesses.len());
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            Err(e) => eprintln!("❌ failed place {link}: {e}"),
        }
    }

    println!();
    Ok(businesses)
}

/// Runs `future`, failing with a clear error if it takes longer than `limit`.
///
/// chromiumoxide's `new_page` can occasionally wait forever, so every page needs a deadline.
async fn within<T>(limit: Duration, future: impl Future<Output = Result<T>>) -> Result<T> {
    timeout(limit, future)
        .await
        .with_context(|| format!("timed out after {}s", limit.as_secs()))?
}

async fn scrape_place(browser: &Browser, url: &str) -> Result<Business> {
    let raw = browser::fetch_place_json(browser, url).await?;
    parser::parse(&raw, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_replaces_spaces_with_plus() {
        let job = SearchJob::new("kitchen modeling toronto");
        assert_eq!(
            "https://www.google.com/maps/search/kitchen+modeling+toronto",
            job.url()
        );
    }
}
