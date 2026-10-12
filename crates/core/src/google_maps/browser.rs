use anyhow::{Context, Result, bail};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use std::time::Duration;
use tokio::task::JoinHandle;

const FEED_SELECTOR: &str = "div[role='feed']";
const FEED_TIMEOUT: Duration = Duration::from_secs(10);
/// After a scroll, how long to wait for Google to load more results before
/// deciding the list has ended.
pub(super) const SCROLL_LOAD_TIMEOUT: Duration = Duration::from_secs(3);
const APP_STATE_ATTEMPTS: usize = 50;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

const FEED_HEIGHT_JS: &str = r#"document.querySelector("div[role='feed']").scrollHeight"#;
const SCROLL_TO_BOTTOM_JS: &str = r#"(() => {
    const el = document.querySelector("div[role='feed']");
    el.scrollTop = el.scrollHeight;
})()"#;

const APP_STATE_JS: &str = r#"(() => {
    const s = window.APP_INITIALIZATION_STATE;
    if (!s || !s[3]) return null;
    for (const key of Object.keys(s[3])) {
        const arr = s[3][key];
        if (!Array.isArray(arr)) continue;
        for (const idx of [6, 5]) {
            const item = arr[idx];
            if (typeof item === 'string' && item.startsWith(")]}'")) return item;
        }
    }
    return null;
})()"#;

/// Starts Chrome and the background task that processes its events.
pub async fn launch(show_browser: bool) -> Result<(Browser, JoinHandle<()>)> {
    let mut builder = BrowserConfig::builder();
    if show_browser {
        builder = builder.with_head();
    }
    let config = builder.build().map_err(anyhow::Error::msg)?;
    let (browser, mut handler) = Browser::launch(config).await?;

    let events = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    Ok((browser, events))
}

/// Google answered with its "unusual traffic" / CAPTCHA page instead of real content.
///
/// A distinct type, so callers can tell "we're blocked" apart from ordinary failures.
#[derive(Debug)]
pub(super) struct Blocked;

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Google is blocking requests (unusual traffic / CAPTCHA page)")
    }
}

impl std::error::Error for Blocked {}

/// Google's block page lives under `/sorry/` and mentions "unusual traffic".
const BLOCKED_JS: &str = r#"(() =>
    location.href.includes("/sorry/") ||
    (document.body?.innerText ?? "").includes("unusual traffic")
)()"#;

/// True if the page is Google's block page. Evaluation errors count as "not blocked".
async fn is_blocked(page: &Page) -> bool {
    match page.evaluate(BLOCKED_JS).await {
        Ok(value) => value.into_value::<bool>().unwrap_or(false),
        Err(_) => false,
    }
}

pub async fn fetch_place_json(browser: &Browser, url: &str) -> Result<String> {
    let page = browser
        .new_page(url)
        .await
        .with_context(|| format!("failed to open page {url}"))?;

    // No `?` here: the page must be closed even when reading fails.
    let result = read_place_state(&page).await;
    page.close().await.ok();
    result
}

async fn read_place_state(page: &Page) -> Result<String> {
    for _ in 0..APP_STATE_ATTEMPTS {
        if let Ok(res) = page.evaluate(APP_STATE_JS).await
            && let Ok(Some(s)) = res.into_value::<Option<String>>()
        {
            return Ok(s);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    if is_blocked(page).await {
        return Err(Blocked.into());
    }
    bail!("APP_INITIALIZATION_STATE not found")
}

pub async fn search_place_links(
    browser: &Browser,
    url: &str,
    max_scrolls: usize,
) -> Result<Vec<String>> {
    let page = browser
        .new_page(url)
        .await
        .with_context(|| format!("failed to open page {url}"))?;

    // No `?` here: the page must be closed even when collecting fails.
    let result = collect_links(&page, max_scrolls).await;
    page.close().await.ok();
    result
}

async fn collect_links(page: &Page, max_scrolls: usize) -> Result<Vec<String>> {
    if !wait_for_selector(page, FEED_SELECTOR, FEED_TIMEOUT).await {
        if is_blocked(page).await {
            return Err(Blocked.into());
        }
        bail!("results feed not found");
    }

    scroll_feed(page, max_scrolls).await?;

    let links = page
        .evaluate(
            r#"Array.from(document.querySelectorAll("div[role='feed'] div[jsaction] > a"))
                .map(a => a.href)
                .filter(h => h)"#,
        )
        .await?
        .into_value()?;

    Ok(links)
}

/// Scrolls the results list up to `max_scrolls` times, stopping early at the end of the list.
async fn scroll_feed(page: &Page, max_scrolls: usize) -> Result<()> {
    let mut height = feed_height(page).await?;

    for _ in 0..max_scrolls {
        page.evaluate(SCROLL_TO_BOTTOM_JS).await?;

        // Google loads the next batch after the scroll, so wait for the list to grow
        // instead of reading the height immediately.
        match wait_for_growth(page, height).await? {
            Some(new_height) => height = new_height,
            None => break,
        }
    }

    Ok(())
}

async fn feed_height(page: &Page) -> Result<u64> {
    Ok(page.evaluate(FEED_HEIGHT_JS).await?.into_value()?)
}

/// Waits for the list to grow past `height`. `None` means it didn't within
/// `SCROLL_LOAD_TIMEOUT`, i.e. there are no more results.
async fn wait_for_growth(page: &Page, height: u64) -> Result<Option<u64>> {
    let start = std::time::Instant::now();
    while start.elapsed() < SCROLL_LOAD_TIMEOUT {
        tokio::time::sleep(POLL_INTERVAL).await;
        let new_height = feed_height(page).await?;
        if new_height > height {
            return Ok(Some(new_height));
        }
    }
    Ok(None)
}

async fn wait_for_selector(page: &Page, selector: &str, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if page.find_element(selector).await.is_ok() {
            return true;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    false
}
