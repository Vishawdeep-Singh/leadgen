//! Progress events, so the caller decides how (or whether) to show them.

/// Receives progress events while scraping.
///
/// Every method has an empty default, so implement only the ones you need.
/// `Sync` because one reporter is shared by all searches running at once.
pub trait Progress: Sync {
    /// A search found `places` place pages to scrape.
    fn search_started(&self, _query: &str, _places: usize) {}

    /// One place page of a search finished, successfully or not.
    fn place_done(&self, _query: &str) {}

    /// A search finished with `found` businesses.
    fn search_finished(&self, _query: &str, _found: usize) {}

    /// Something failed, but the run continues.
    fn warn(&self, _message: &str) {}
}

/// Ignores all progress events.
pub struct NoProgress;

impl Progress for NoProgress {}
