use std::collections::HashSet;

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Serialize)]
pub struct Business {
    pub title: String,
    #[serde(serialize_with = "join_with_semicolons")]
    pub categories: Vec<String>,
    pub address: String,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub rating: Option<f64>,
    pub review_count: Option<u64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub place_id: String,
    pub link: String,
}

/// Writes a list as one value, e.g. `["A", "B"]` → `"A; B"`.
fn join_with_semicolons<S: Serializer>(items: &[String], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&items.join("; "))
}

/// Keeps the first business for each place_id.
pub fn dedupe(businesses: Vec<Business>) -> Vec<Business> {
    let mut seen = HashSet::new();
    businesses
        .into_iter()
        .filter(|b| seen.insert(b.place_id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn business(title: &str, place_id: &str) -> Business {
        Business {
            title: title.into(),
            categories: vec![],
            address: String::new(),
            phone: None,
            website: None,
            rating: None,
            review_count: None,
            latitude: None,
            longitude: None,
            place_id: place_id.into(),
            link: String::new(),
        }
    }

    #[test]
    fn dedupe_keeps_first_of_each_place_id() {
        let leads = vec![business("A", "x"), business("B", "y"), business("C", "x")];

        let titles: Vec<String> = dedupe(leads).into_iter().map(|b| b.title).collect();

        assert_eq!(titles, ["A", "B"]);
    }
}
