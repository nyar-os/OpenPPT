//! Static PDF projection request plumbing.
//!
//! The projection itself lives in `document-presentation-pdf`: this module only
//! maps the export request, applies the loss policy and publishes the bytes.
//! One deck slide becomes one PDF page sized from the deck in points.

use std::path::Path;

use document_presentation::authored::Deck;
use document_presentation_pdf::{export_pdf_bytes, PdfLoss};
use serde_json::{Value, json};

use crate::ServiceError;

pub(crate) use document_presentation_pdf::{LOSS_CHART, LOSS_FORMULA, LOSS_GLYPH, LOSS_GROUP, LOSS_IMAGE, LOSS_MEDIA};

/// Writes `deck` as a PDF file and reports what the projection dropped.
pub(crate) fn export(deck: &Deck, request: &Value) -> Result<Value, ServiceError> {
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

    let export = export_pdf_bytes(deck).map_err(|error| ServiceError::new(error.code, error.message))?;
    if loss_policy == "error" && !export.losses.is_empty() {
        return Err(ServiceError::new(
            "export/loss-exceeded",
            format!("the pdf projection would drop {} construct(s)", export.losses.len()),
        ));
    }
    crate::write_atomically(&export.bytes, Path::new(target), overwrite)?;

    Ok(json!({
        "revision": deck.revision(),
        "format": "pdf",
        "target": target,
        "bytes": export.bytes.len(),
        "editable": false,
        "losses": losses_json(&export.losses),
        "diagnostics": [],
    }))
}

/// Serialises the loss list with the stable camelCase keys the products report.
fn losses_json(losses: &[PdfLoss]) -> Vec<Value> {
    losses
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
        .collect()
}
