//! PresentationML export request handling.
//!
//! The package writer lives in `document-presentation-pptx`; this module keeps
//! the product contract: target routing, the loss policy and the report shape.

use std::path::Path;

use document_presentation::authored::Deck;
use document_presentation_pptx::export_presentation_bytes;
use serde_json::{Value, json};

use crate::ServiceError;

pub use document_presentation_pptx::{LOSS_CHART, LOSS_FORMULA, LOSS_GROUP, LOSS_IMAGE, LOSS_MEDIA};

/// Writes the deck as a PresentationML package to `request.target`.
///
/// The request carries `format` (`pptx`, `ppsx`, `potx` or `pdf`), `target`,
/// `overwrite` and an optional `lossPolicy` (`warn` by default, `error` refuses
/// to write when anything would be dropped). `pdf` is a static projection and
/// is written by [`crate::pdf`].
pub fn export(deck: &Deck, request: &Value) -> Result<Value, ServiceError> {
    let format = request.get("format").and_then(Value::as_str).unwrap_or("pptx");
    if format == "pdf" {
        return crate::pdf::export(deck, request);
    }
    let format = document_presentation_pptx::Format::parse(format).ok_or_else(|| {
        ServiceError::new(
            "format/unsupported",
            format!("`{format}` is not a presentation export target; use pptx, ppsx, potx or pdf"),
        )
    })?;
    let target = request
        .get("target")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ServiceError::new("session/argument-invalid", "`target` must be a non-empty file path"))?;
    let overwrite = request.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    let loss_policy = request.get("lossPolicy").and_then(Value::as_str).unwrap_or("warn");
    if !matches!(loss_policy, "warn" | "error") {
        return Err(ServiceError::new("session/argument-invalid", "`lossPolicy` must be `warn` or `error`"));
    }

    let written =
        export_presentation_bytes(deck, format).map_err(|error| ServiceError::new("format/invalid-package", error.message))?;
    let losses: Vec<Value> = written
        .losses
        .iter()
        .map(|loss| {
            json!({
                "code": loss.code,
                "severity": "warning",
                "slideId": loss.slide,
                "elementId": loss.element,
                "message": loss.message,
            })
        })
        .collect();
    if loss_policy == "error" && !losses.is_empty() {
        return Err(ServiceError::new(
            "export/loss-exceeded",
            format!("the {} projection would drop {} construct(s)", format.as_str(), losses.len()),
        ));
    }
    crate::write_atomically(&written.bytes, Path::new(target), overwrite)?;

    Ok(json!({
        "revision": deck.revision(),
        "format": format.as_str(),
        "target": target,
        "bytes": written.bytes.len(),
        "editable": true,
        "losses": losses,
        "diagnostics": [],
    }))
}

#[cfg(test)]
mod tests {
    use document_formats::import::pptx::import_pptx_bytes;
    use document_presentation::authored::{
        ChartData, ChartElementInput, ChartSeries, ChartType, DeckCreateOptions, SlideAddOptions,
    };

    use super::*;

    fn lossy_deck() -> Deck {
        let mut deck = Deck::create(DeckCreateOptions::default());
        deck.transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_chart(
                &slide,
                ChartElementInput {
                    chart_type: ChartType::Column,
                    data: ChartData {
                        categories: vec!["Q1".to_string()],
                        series: vec![ChartSeries { name: "Sales".to_string(), values: vec![1.0] }],
                    },
                    transform: None,
                    alt: "Revenue".to_string(),
                },
            )?;
            Ok(())
        })
        .expect("chart deck builds");
        deck
    }

    fn temp_target(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join("open-ppt-export");
        std::fs::create_dir_all(&directory).expect("temp directory");
        directory.join(name)
    }

    #[test]
    fn export_refuses_loss_under_error_policy() {
        let target = temp_target("lossy.pptx");
        let error = export(
            &lossy_deck(),
            &json!({ "format": "pptx", "target": target.display().to_string(), "overwrite": true, "lossPolicy": "error" }),
        )
        .expect_err("a lossy projection must fail under the error policy");
        assert_eq!(error.code, "export/loss-exceeded");
        assert!(!target.exists(), "nothing is written when the error policy refuses");
    }

    #[test]
    fn export_writes_a_reimportable_package() {
        let target = temp_target("notes.pptx");
        let report =
            export(&lossy_deck(), &json!({ "format": "pptx", "target": target.display().to_string(), "overwrite": true }))
                .expect("package writes");
        assert_eq!(report["format"], json!("pptx"));
        assert_eq!(report["losses"][0]["code"], json!(LOSS_CHART));
        let bytes = std::fs::read(&target).expect("written package");
        let imported = import_pptx_bytes("notes.pptx", &bytes).expect("the written package imports");
        assert_eq!(imported.slides.len(), 1);
    }

    #[test]
    fn unknown_targets_are_rejected() {
        let error = export(&lossy_deck(), &json!({ "format": "odp", "target": "deck.odp" })).expect_err("odp is unsupported");
        assert_eq!(error.code, "format/unsupported");
    }
}
