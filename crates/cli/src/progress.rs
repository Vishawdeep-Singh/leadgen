//! Terminal progress bars: one line per search.

use std::collections::HashMap;
use std::sync::Mutex;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use leadgen_core::progress::Progress;

/// Shows one progress bar per search, with warnings printed above the bars.
pub struct Bars {
    multi: MultiProgress,
    // Mutex: events arrive through `&self` from several searches at once.
    bars: Mutex<HashMap<String, ProgressBar>>,
}

impl Bars {
    pub fn new() -> Self {
        Self {
            multi: MultiProgress::new(),
            bars: Mutex::new(HashMap::new()),
        }
    }

    /// Prints a line above the bars, or straight to stderr when bars are hidden
    /// (e.g. output redirected to a file or running under cron).
    fn line(&self, text: String) {
        if self.multi.is_hidden() {
            eprintln!("{text}");
        } else {
            self.multi.println(text).ok();
        }
    }
}

impl Progress for Bars {
    fn search_started(&self, query: &str, places: usize) {
        let style = ProgressStyle::with_template("{prefix:32!} [{bar:25}] {pos:>2}/{len:2} {msg}")
            .expect("progress template is valid")
            .progress_chars("█░");

        let bar = self.multi.add(ProgressBar::new(places as u64));
        bar.set_style(style);
        bar.set_prefix(query.to_string());

        self.bars
            .lock()
            .expect("progress lock poisoned")
            .insert(query.to_string(), bar);
    }

    fn place_done(&self, query: &str) {
        if let Some(bar) = self.bars.lock().expect("progress lock poisoned").get(query) {
            bar.inc(1);
        }
    }

    fn search_finished(&self, query: &str, found: usize) {
        if let Some(bar) = self.bars.lock().expect("progress lock poisoned").get(query) {
            bar.finish_with_message(format!("✅ {found} found"));
        }
        if self.multi.is_hidden() {
            eprintln!("✅ {query}: {found} found");
        }
    }

    fn warn(&self, message: &str) {
        self.line(format!("⚠️  {message}"));
    }
}
