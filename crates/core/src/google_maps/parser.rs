use crate::business::Business;
use anyhow::{Context, Result};
use serde_json::Value;

pub fn parse(raw: &str, url: &str) -> Result<Business> {
    let json: Value = serde_json::from_str(raw.trim_start_matches(")]}'").trim())?;
    let d = json.get(6).context("place data missing")?;

    let title = str_at(d, &[11]);

    let rating = at(d, &[4, 7]).and_then(Value::as_f64);

    let review_count = at(d, &[4, 8]).and_then(Value::as_u64);

    let latitude = at(d, &[9, 2]).and_then(Value::as_f64);

    let longitude = at(d, &[9, 3]).and_then(Value::as_f64);

    Ok(Business {
        address: str_at(d, &[18])
            .trim_start_matches(&format!("{title},"))
            .trim()
            .to_string(),
        categories: at(d, &[13])
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        website: opt_str_at(d, &[7, 0]),
        phone: opt_str_at(d, &[178, 0, 0]),
        rating,
        review_count,
        latitude,
        longitude,
        place_id: str_at(d, &[78]),
        link: url.to_string(),
        title,
    })
}
fn at<'a>(v: &'a Value, path: &[usize]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, &i| v.get(i))
}

fn str_at(v: &Value, path: &[usize]) -> String {
    at(v, path)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn opt_str_at(v: &Value, path: &[usize]) -> Option<String> {
    at(v, path)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = include_str!("../../tests/fixtures/place_full.txt");

    #[test]
    fn parses_real_place() {
        let b = parse(FULL, "https://example.com").unwrap();

        assert_eq!(b.title, "Easy Bathroom Renovation Toronto");
        assert_eq!(
            b.address,
            "2 St Clair Ave W 18th floor, Toronto, ON M4V 1L5, Canada"
        );
        assert_eq!(b.phone.as_deref(), Some("+1 416-747-5996"));
        assert_eq!(b.website.as_deref(), Some("https://easybathroom.ca/"));
        assert_eq!(b.rating, Some(5.0));
        assert_eq!(b.review_count, Some(104));
        assert_eq!(b.latitude, Some(43.6883013));
        assert_eq!(b.longitude, Some(-79.3946163));
        assert_eq!(b.categories, ["Bathroom remodeler", "General contractor"]);
        assert_eq!(b.place_id, "ChIJEZkhN_MzK4gRougE5jko4_A");
        assert_eq!(b.link, "https://example.com");
    }
}
