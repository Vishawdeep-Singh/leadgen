use anyhow::{Context, Result, bail};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use std::time::Duration;
use tokio::task::JoinHandle;

const FEED_SELECTOR: &str = "div[role='feed']";
const FEED_TIMEOUT: Duration = Duration::from_secs(10);
const SCROLL_WAIT_START: Duration = Duration::from_millis(100);
const SCROLL_WAIT_MAX: Duration = Duration::from_secs(2);
const SCROLL_BACKOFF: f64 = 1.5;
const APP_STATE_ATTEMPTS: usize = 50;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

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

pub async fn fetch_place_json(browser: &Browser, url: &str) -> Result<String> {
    let page = browser
        .new_page(url)
        .await
        .with_context(|| format!("failed to open page {}", url))?;

    let mut raw = None;
    for _ in 0..APP_STATE_ATTEMPTS {
        if let Ok(res) = page.evaluate(APP_STATE_JS).await
            && let Ok(Some(s)) = res.into_value::<Option<String>>()
        {
            raw = Some(s);
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    page.close().await.ok();

    raw.context("APP_INITIALIZATION_STATE not found")
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

async fn scroll_feed(page: &Page, max_scrolls: usize) -> Result<()> {
    let scroll_js = r#"(() => {
        const el = document.querySelector("div[role='feed']");
        el.scrollTop = el.scrollHeight;
        return el.scrollHeight;
    })()"#;

    let mut last_height = 0;
    let mut wait = SCROLL_WAIT_START;

    for _ in 0..max_scrolls {
        let height: u64 = page.evaluate(scroll_js).await?.into_value()?;
        if height == last_height {
            break;
        }
        last_height = height;

        tokio::time::sleep(wait).await;
        wait = wait.mul_f64(SCROLL_BACKOFF).min(SCROLL_WAIT_MAX);
    }

    Ok(())
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
