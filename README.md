# leadgen

Scrape business leads from Google Maps into a CSV file.

Give it searches like `"kitchen remodeling"` and a city, and it collects each business's
name, phone, website, address, rating, review count, coordinates and categories.

```bash
leadgen "kitchen remodeling" "appliance repair" --location toronto
```

```
🚀 Starting scraper: 2 searches, 2 at a time...

kitchen remodeling toronto       [█████████████████████████] 27/27 ✅ 27 found
appliance repair toronto         [██████████░░░░░░░░░░░░░░░] 16/40

💾 Saved 67 leads to leads.csv
```

## Requirements

- **Rust 1.88 or newer** ([install](https://rustup.rs))
- **Google Chrome or Chromium**: leadgen drives a real browser in the background

## Install

```bash
git clone <this repo>
cd leadgen
cargo install --path crates/cli     # installs the `leadgen` command
```

Or run it without installing: replace `leadgen` with `cargo run --release --` in the examples below.

## Usage

```bash
# A few searches, all in one city
leadgen "plumber" "electrician" -l toronto

# Many searches from a file (one per line)
leadgen -f queries.txt -l toronto -o toronto_leads.csv

# Watch Chrome while it works (for debugging)
leadgen "plumber" -l toronto -c 1 --show-browser
```

### Query file

One search per line. Blank lines and lines starting with `#` are skipped.
`--location` is added to the end of every line.

```text
# queries.txt
kitchen remodeling
appliance repair
washing machine repair
```

### Options

| Option | Default | Description |
|---|---|---|
| `[QUERIES]...` | | Searches, e.g. `"kitchen remodeling"` |
| `-f, --file` | | File with one query per line |
| `-l, --location` | | City added to every query, e.g. `toronto` |
| `-o, --output` | `leads.csv` | Where to save the leads |
| `-c, --concurrency` | `2` | Searches scraped at the same time |
| `--place-concurrency` | `3` | Place pages opened at the same time within each search |
| `--max-scrolls` | `5` | Scrolls of the results list; more scrolls find more places |
| `--page-timeout` | `30` | Seconds before one page is abandoned (only that page is lost) |
| `--show-browser` | off | Show the Chrome window |

Run `leadgen --help` for the full list.

## Output

A CSV with one row per business. The same business found by several searches appears once.

| Column | Example |
|---|---|
| `title` | Easy Bathroom Renovation Toronto |
| `categories` | Bathroom remodeler; General contractor |
| `address` | 2 St Clair Ave W 18th floor, Toronto, ON M4V 1L5, Canada |
| `phone` | +1 416-747-5996 |
| `website` | https://easybathroom.ca/ |
| `rating` | 5 |
| `review_count` | 104 |
| `latitude`, `longitude` | 43.6883013, -79.3946163 |
| `place_id` | ChIJEZkhN_MzK4gRougE5jko4_A |
| `link` | the Google Maps page |

Missing values (no phone, no website, …) are empty cells.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Finished |
| `1` | Failed, or Google blocked the run (leads found before the block are still saved) |
| `2` | Invalid arguments |

## Avoiding blocks

Google limits automated traffic. If it sees too many requests, it shows a CAPTCHA and leadgen stops:

```
⚠️  Google is blocking requests (unusual traffic / CAPTCHA). Stopping now. ...
```

- **Wait a few hours** before trying again. Requests during a block can make it last longer.
- **Keep concurrency low.** Total pages open at once is `--concurrency × --place-concurrency`;
  the defaults (2 × 3 = 6) are a safe start. 15+ pages at once gets blocked quickly.
- Don't run many big scrapes back to back.

Automated scraping is against Google Maps' Terms of Service. For commercial use, consider the
official [Places API](https://developers.google.com/maps/documentation/places/web-service).

## Development

```bash
cargo fmt --all -- --check                  # formatting
cargo clippy --all-targets -- -D warnings   # lints
cargo test --all                            # all tests, offline (none contact Google)
```

Run all three before committing.

### Project layout

```
crates/
  core/                     leadgen_core: the scraping library (no terminal output)
    src/google_maps.rs        Config, SearchJob, run(): coordinates a scrape
    src/google_maps/browser.rs  Chrome: opening pages, scrolling, block detection
    src/google_maps/parser.rs   Google's JSON → Business (tested with a real saved response)
    src/business.rs           Business struct + dedupe
    src/export.rs             CSV writing
    src/progress.rs           Progress trait: core reports events, the caller shows them
    tests/fixtures/           a real Google response used by the parser tests
  cli/                      leadgen: the command-line program
    src/main.rs               argument parsing (clap)
    src/progress.rs           progress bars (indicatif)
    tests/cli.rs              runs the real binary with bad input
```

### When Google changes its page

Google's data has no field names, only positions (e.g. the phone is at `[178][0][0]`),
so a Google update can silently empty a column. If a column suddenly comes back empty:

1. Save a fresh response: open any business on Google Maps in Chrome, open DevTools
   (`Cmd+Option+J`), paste the `APP_STATE_JS` snippet from `browser.rs` wrapped in `copy(...)`,
   and paste the clipboard into `crates/core/tests/fixtures/place_full.txt`.
2. Look at the fields with `jq`:
   ```bash
   tail -n +2 crates/core/tests/fixtures/place_full.txt | jq '.[6] | {title: .[11], phone: .[178][0][0]}'
   ```
3. Update the paths in `crates/core/src/google_maps/parser.rs` and the expected values in its
   `parses_real_place` test, then run `cargo test`.
