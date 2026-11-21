//! Per-slide talking points projected from committed text.
//!
//! Points are derived from the text elements of each slide, so an agenda or a
//! speaker outline can be produced without a render call and without changing
//! the deck model. Text bound to the `title` placeholder is skipped because the
//! slide already reports it as its title.

use document_presentation::authored::{Deck, ElementContent};
use serde_json::{Value, json};

use crate::ServiceError;

/// Default cap on the number of points reported per slide.
const DEFAULT_MAX_POINTS: usize = 6;
/// Upper bound accepted for `maxPointsPerSlide`.
const MAX_POINTS_LIMIT: usize = 20;

/// Lists one entry per slide with its title, notes, hidden flag and points.
///
/// The request accepts `maxPointsPerSlide` (1 to 20, default 6). Points keep
/// their reading order, are trimmed, empty lines are dropped, duplicates are
/// removed and lines that repeat the slide title are skipped.
pub fn outline(deck: &Deck, request: &Value) -> Result<Value, ServiceError> {
    let max_points = max_points(request)?;
    let slides = deck
        .slides()
        .iter()
        .enumerate()
        .map(|(index, slide)| {
            let title = slide.title();
            let mut points: Vec<String> = Vec::new();
            for element in &slide.elements {
                if element.placeholder.as_ref().is_some_and(|placeholder| placeholder.name == "title") {
                    continue;
                }
                let ElementContent::Text(text) = &element.content
                else {
                    continue;
                };
                for paragraph in &text.paragraphs {
                    for line in paragraph.text().split('\n') {
                        if points.len() >= max_points {
                            break;
                        }
                        let line = line.trim();
                        if line.is_empty() || line == title || points.iter().any(|point| point == line) {
                            continue;
                        }
                        points.push(line.to_string());
                    }
                    if points.len() >= max_points {
                        break;
                    }
                }
                if points.len() >= max_points {
                    break;
                }
            }
            json!({
                "id": slide.id.as_str(),
                "index": index,
                "title": title,
                "hidden": slide.hidden,
                "elements": slide.elements.len(),
                "notes": slide.notes,
                "points": points,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({ "revision": deck.revision(), "slides": slides }))
}

/// Reads `maxPointsPerSlide` and refuses values the projection cannot honour.
fn max_points(request: &Value) -> Result<usize, ServiceError> {
    match request.get("maxPointsPerSlide") {
        None | Some(Value::Null) => Ok(DEFAULT_MAX_POINTS),
        Some(value) => {
            let number = value.as_u64().and_then(|number| usize::try_from(number).ok()).ok_or_else(|| {
                ServiceError::new("session/argument-invalid", "`maxPointsPerSlide` must be a positive integer")
            })?;
            if number == 0 || number > MAX_POINTS_LIMIT {
                return Err(ServiceError::new(
                    "session/argument-invalid",
                    format!("`maxPointsPerSlide` must be between 1 and {MAX_POINTS_LIMIT}"),
                ));
            }
            Ok(number)
        }
    }
}

#[cfg(test)]
mod tests {
    use document_presentation::authored::{
        DeckCreateOptions, PlaceholderRef, SlideAddOptions, TextContent, TextElementInput, Transform,
    };

    use super::*;

    fn deck_with_points() -> (Deck, String) {
        let mut deck = Deck::create(DeckCreateOptions::default());
        let slide = deck
            .transaction(None, |edit| {
                let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
                edit.add_text(
                    &slide,
                    TextElementInput {
                        text: TextContent::plain("Roadmap"),
                        transform: Some(Transform::from_points(40.0, 40.0, 400.0, 60.0).unwrap()),
                        alt: None,
                        placeholder: Some(PlaceholderRef { layout: None, name: "title".to_string() }),
                    },
                )?;
                edit.add_text(
                    &slide,
                    TextElementInput {
                        text: TextContent::plain("Ship the editor\nAdd PDF export\nShip the editor"),
                        transform: Some(Transform::from_points(40.0, 120.0, 400.0, 200.0).unwrap()),
                        alt: None,
                        placeholder: None,
                    },
                )?;
                edit.set_slide_notes(&slide, Some("Mention the schedule".to_string()))?;
                Ok(slide)
            })
            .expect("outline deck builds");
        (deck, slide.as_str().to_string())
    }

    #[test]
    fn outline_reports_body_lines_without_titles_or_duplicates() {
        let (deck, slide) = deck_with_points();
        let report = outline(&deck, &json!({})).expect("outline projects");

        assert_eq!(report["revision"], deck.revision());
        let entry = &report["slides"][0];
        assert_eq!(entry["id"].as_str(), Some(slide.as_str()));
        assert_eq!(entry["title"], "Roadmap");
        assert_eq!(entry["elements"], 2);
        assert_eq!(entry["notes"], "Mention the schedule");
        assert_eq!(entry["points"], json!(["Ship the editor", "Add PDF export"]));
    }

    #[test]
    fn outline_caps_points_and_rejects_unusable_limits() {
        let (deck, _) = deck_with_points();
        let report = outline(&deck, &json!({ "maxPointsPerSlide": 1 })).expect("the cap applies");
        assert_eq!(report["slides"][0]["points"], json!(["Ship the editor"]));

        for request in
            [json!({ "maxPointsPerSlide": 0 }), json!({ "maxPointsPerSlide": 21 }), json!({ "maxPointsPerSlide": "many" })]
        {
            let error = outline(&deck, &request).expect_err("an unusable limit is refused");
            assert_eq!(error.code, "session/argument-invalid");
        }
    }
}
