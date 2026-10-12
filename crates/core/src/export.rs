use crate::business::Business;
use anyhow::{Context, Result};
use csv::Writer;
use std::fs::File;
use std::path::Path;

/// Writes the businesses to a CSV file, one row each.
///
/// # Errors
/// Fails if the file can't be created or written.
pub fn save_to_csv(businesses: &[Business], path: &Path) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let mut writer = Writer::from_writer(file);

    for business in businesses {
        writer.serialize(business)?;
    }

    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn business() -> Business {
        Business {
            title: "Joe's Kitchen".into(),
            categories: vec!["Kitchen remodeler".into(), "Contractor".into()],
            address: "123 Queen St".into(),
            phone: None,
            website: Some("https://joes.ca".into()),
            rating: Some(4.5),
            review_count: None,
            latitude: None,
            longitude: None,
            place_id: "abc".into(),
            link: "https://maps".into(),
        }
    }

    #[test]
    fn writes_header_and_row() {
        let mut writer = Writer::from_writer(vec![]);
        writer.serialize(business()).unwrap();
        let csv = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        assert_eq!(
            csv,
            "title,categories,address,phone,website,rating,review_count,latitude,longitude,place_id,link\n\
             Joe's Kitchen,Kitchen remodeler; Contractor,123 Queen St,,https://joes.ca,4.5,,,,abc,https://maps\n"
        );
    }
}
