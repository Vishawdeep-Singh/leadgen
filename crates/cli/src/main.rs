use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use leadgen_core::export;
use leadgen_core::google_maps::{Config, SearchJob, run};

/// Scrape business leads from Google Maps into a CSV file.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Search queries, e.g. "kitchen remodeling"
    #[arg(required_unless_present = "file")]
    queries: Vec<String>,

    /// File with one query per line (blank lines and lines starting with # are skipped)
    #[arg(short, long)]
    file: Option<PathBuf>,

    /// City added to the end of every query, e.g. "toronto"
    #[arg(short, long)]
    location: Option<String>,

    /// Where to save the leads
    #[arg(short, long, default_value = "leads.csv")]
    output: PathBuf,

    /// Searches scraped at the same time
    #[arg(short, long, default_value_t = Config::default().concurrency)]
    concurrency: NonZeroUsize,

    /// Place pages opened at the same time within each search
    #[arg(long, default_value_t = Config::default().place_concurrency)]
    place_concurrency: NonZeroUsize,

    /// Times to scroll the results list; more scrolls find more places
    #[arg(long, default_value_t = Config::default().max_scrolls)]
    max_scrolls: usize,

    /// Seconds before a single page is abandoned (only that page is lost)
    #[arg(long, default_value_t = Config::default().page_timeout.as_secs())]
    page_timeout: u64,

    /// Show the Chrome window (useful for debugging)
    #[arg(long)]
    show_browser: bool,
}

impl Args {
    /// Collects queries from the arguments and `--file`, adding `--location` to each.
    fn jobs(&self) -> Result<Vec<SearchJob>> {
        let mut queries = self.queries.clone();

        if let Some(path) = &self.file {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            queries.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(String::from),
            );
        }

        if queries.is_empty() {
            bail!("no queries given: pass them as arguments or with --file");
        }

        let jobs = queries
            .into_iter()
            .map(|query| match &self.location {
                Some(location) => SearchJob::new(format!("{query} {location}")),
                None => SearchJob::new(query),
            })
            .collect();
        Ok(jobs)
    }

    fn config(&self) -> Config {
        Config {
            concurrency: self.concurrency,
            place_concurrency: self.place_concurrency,
            max_scrolls: self.max_scrolls,
            page_timeout: Duration::from_secs(self.page_timeout),
            show_browser: self.show_browser,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let jobs = args.jobs()?;
    let config = args.config();

    println!(
        "🚀 Starting scraper: {} searches, {} at a time...\n",
        jobs.len(),
        config.concurrency
    );

    let businesses = run(jobs, &config).await?;
    export::save_to_csv(&businesses, &args.output)?;
    println!("\n✅ Total prospects scraped: {}", businesses.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("leadgen").chain(args.iter().copied()))
    }

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }

    #[test]
    fn location_is_added_to_every_query() {
        let args = parse(&["plumber", "electrician", "-l", "toronto"]).unwrap();

        let queries: Vec<String> = args.jobs().unwrap().into_iter().map(|j| j.query).collect();

        assert_eq!(queries, ["plumber toronto", "electrician toronto"]);
    }

    #[test]
    fn queries_are_required_without_file() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn zero_concurrency_is_rejected() {
        assert!(parse(&["plumber", "-c", "0"]).is_err());
    }
}
