mod browser;
mod parser;

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use chromiumoxide::Browser;
use futures::stream::{self, StreamExt};
use tokio::time::timeout;

use crate::business::{Business, dedupe};
use crate::progress::Progress;
use browser::Blocked;

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

/// What a run produced.
#[derive(Debug)]
pub struct Outcome {
    /// Every business found, without duplicates.
    pub businesses: Vec<Business>,
    /// Google started blocking requests, so the run stopped early.
    /// `businesses` still holds everything scraped before that.
    pub blocked: bool,
}

pub async fn run(
    jobs: Vec<SearchJob>,
    config: &Config,
    progress: &dyn Progress,
) -> Result<Outcome> {
    let (mut browser, events) = browser::launch(config.show_browser).await?;

    let scraper = Scraper {
        browser: &browser,
        config,
        progress,
        blocked: BlockState::default(),
    };

    let results: Vec<Vec<Business>> = stream::iter(jobs)
        .map(|job| scraper.scrape_job(job))
        .buffer_unordered(config.concurrency.get())
        .collect()
        .await;
    let blocked = scraper.blocked.into_inner();

    // Leads are already scraped; a failed shutdown shouldn't lose them.
    browser.close().await.ok();
    events.await.ok();

    Ok(Outcome {
        businesses: dedupe(results.into_iter().flatten().collect()),
        blocked,
    })
}

/// Everything the searches of one run share.
struct Scraper<'a> {
    browser: &'a Browser,
    config: &'a Config,
    progress: &'a dyn Progress,
    /// Set once Google blocks us; all remaining work is skipped.
    blocked: BlockState,
}

impl Scraper<'_> {
    /// Scrapes one search. On failure, reports it and returns nothing so other searches continue.
    async fn scrape_job(&self, job: SearchJob) -> Vec<Business> {
        if self.is_blocked() {
            return Vec::new();
        }

        match self.scrape(&job).await {
            Ok(businesses) => {
                self.progress.search_finished(&job.query, businesses.len());
                businesses
            }
            Err(e) => {
                self.report(&format!("search \"{}\" failed", job.query), &e);
                Vec::new()
            }
        }
    }

    async fn scrape(&self, job: &SearchJob) -> Result<Vec<Business>> {
        let config = self.config;

        // Scrolling waits for each batch to load, so the results page gets extra time per scroll.
        let scroll_budget = browser::SCROLL_LOAD_TIMEOUT * config.max_scrolls as u32;
        let links = within(
            config.page_timeout + scroll_budget,
            browser::search_place_links(self.browser, &job.url(), config.max_scrolls),
        )
        .await?;
        self.progress.search_started(&job.query, links.len());

        let mut places = stream::iter(links)
            .map(|link| async move {
                // Places queued before a block are skipped instead of hitting Google again.
                if self.is_blocked() {
                    return (link, Err(Blocked.into()));
                }
                let result = within(config.page_timeout, scrape_place(self.browser, &link)).await;
                (link, result)
            })
            .buffer_unordered(config.place_concurrency.get());

        let mut businesses = Vec::new();

        while let Some((link, result)) = places.next().await {
            match result {
                Ok(b) => businesses.push(b),
                Err(e) => self.report(&format!("place \"{}\" failed", place_name(&link)), &e),
            }
            self.progress.place_done(&job.query);
        }

        Ok(businesses)
    }

    fn is_blocked(&self) -> bool {
        self.blocked.is_blocked()
    }

    fn report(&self, what: &str, error: &anyhow::Error) {
        self.blocked.report(self.progress, what, error);
    }
}

/// Remembers whether Google has blocked this run.
#[derive(Debug, Default)]
struct BlockState(AtomicBool);

impl BlockState {
    fn is_blocked(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn into_inner(self) -> bool {
        self.0.into_inner()
    }

    /// Reports a failure. A block is announced once and stops the run; once blocked,
    /// other errors are just side effects of the block, so they stay quiet.
    fn report(&self, progress: &dyn Progress, what: &str, error: &anyhow::Error) {
        if error.is::<Blocked>() {
            // `swap` returns the old value, so only the first block prints.
            if !self.0.swap(true, Ordering::Relaxed) {
                progress.warn(
                    "Google is blocking requests (unusual traffic / CAPTCHA). \
                     Stopping now. Wait a while, then retry with a lower --concurrency.",
                );
            }
        } else if !self.is_blocked() {
            progress.warn(&format!("{what}: {error:#}"));
        }
    }
}

/// The readable part of a place URL: `.../place/Joe's+Kitchen/data=...` → `Joe's Kitchen`.
fn place_name(link: &str) -> String {
    link.split("/place/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(link)
        .replace('+', " ")
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
    fn place_name_is_read_from_url() {
        let link = "https://www.google.com/maps/place/Max+Appliance+Repair/data=!4m7!3m6?hl=en";
        assert_eq!(place_name(link), "Max Appliance Repair");
    }

    /// Collects warnings so a test can check what would have been printed.
    #[derive(Default)]
    struct Recorder(std::sync::Mutex<Vec<String>>);

    impl Progress for Recorder {
        fn warn(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    #[test]
    fn block_is_announced_once_and_silences_later_errors() {
        let recorder = Recorder::default();
        let state = BlockState::default();

        state.report(&recorder, "place A failed", &anyhow::anyhow!("timed out"));
        state.report(&recorder, "place B failed", &Blocked.into());
        state.report(&recorder, "place C failed", &Blocked.into());
        state.report(&recorder, "place D failed", &anyhow::anyhow!("timed out"));

        let warnings = recorder.0.lock().unwrap();
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0], "place A failed: timed out");
        assert!(warnings[1].contains("Google is blocking"));
        assert!(state.is_blocked());
    }

    #[test]
    fn block_is_detected_through_added_context() {
        let state = BlockState::default();
        let error = anyhow::Error::from(Blocked).context("while scrolling");

        state.report(&Recorder::default(), "search failed", &error);

        assert!(state.is_blocked());
    }

    #[test]
    fn url_replaces_spaces_with_plus() {
        let job = SearchJob::new("kitchen modeling toronto");
        assert_eq!(
            "https://www.google.com/maps/search/kitchen+modeling+toronto",
            job.url()
        );
    }
}
