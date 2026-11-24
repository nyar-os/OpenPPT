//! Static PDF projection request plumbing.
//!
//! The projection itself lives in `document-presentation-pdf`: this module only
//! maps the export request, applies the loss policy and publishes the bytes.
//! One deck slide becomes one PDF page sized from the deck in points.

use std::path::Path;

use document_presentation::authored::Deck;
use document_presentation_pdf::{PdfLoss, export_pdf_bytes};
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
        .ok_or_else(|| ServiceError::argument("`target` must be a non-empty file path"))?;
    let overwrite = request.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
    let loss_policy = request.get("lossPolicy").and_then(Value::as_str).unwrap_or("warn");
    if !matches!(loss_policy, "warn" | "error") {
        return Err(ServiceError::argument("`lossPolicy` must be `warn` or `error`"));
    }

    let export = export_pdf_bytes(deck).map_err(|error| ServiceError::engine(error.code, error.message))?;
    if loss_policy == "error" && !export.losses.is_empty() {
        return Err(ServiceError::loss_exceeded(export.losses.len(), loss_policy));
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

#[cfg(test)]
mod tests {
    use std::fs;

    use document_presentation::authored::{
        ChartData, ChartElementInput, ChartSeries, ChartType, DeckCreateOptions, Fill, FormulaElementInput, ImageElementInput,
        Line, MediaKind, ShapeElementInput, SlideAddOptions, TableCell, TableElementInput, TableRow, TextContent,
        TextElementInput, Transform, points_to_emu,
    };
    use document_presentation_pdf::{LOSS_CHART, LOSS_FORMULA, LOSS_IMAGE, LOSS_MEDIA, export_pdf_bytes};
    use serde_json::json;

    use super::*;

    fn count(haystack: &[u8], needle: &[u8]) -> usize {
        haystack.windows(needle.len()).filter(|window| *window == needle).count()
    }

    /// One 1x1 opaque red pixel encoded as PNG.
    fn tiny_png() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = 1u32.to_be_bytes().to_vec();
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        png.extend_from_slice(&png_chunk(b"IHDR", &header));
        let row = [0u8, 255, 0, 0];
        let mut idat = vec![0x78, 0x01, 0x01, 4, 0, 0xFB, 0xFF];
        idat.extend_from_slice(&row);
        let mut checksum = 1u32;
        for &byte in &row {
            checksum = adler32(checksum, byte);
        }
        idat.extend_from_slice(&checksum.to_be_bytes());
        png.extend_from_slice(&png_chunk(b"IDAT", &idat));
        png.extend_from_slice(&png_chunk(b"IEND", &[]));
        png
    }

    fn adler32(state: u32, byte: u8) -> u32 {
        let a = (state & 0xFFFF) + u32::from(byte);
        let b = (state >> 16) + a;
        (b % 65521) << 16 | (a % 65521)
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut checked = kind.to_vec();
        checked.extend_from_slice(data);
        out.extend_from_slice(&crc32(&checked).to_be_bytes());
        out
    }

    /// Builds one slide holding every construct the projection handles plus the
    /// three it reports as loss.
    fn medium_deck() -> Deck {
        let mut deck =
            Deck::create(DeckCreateOptions { title: Some("PDF medium".to_string()), ..DeckCreateOptions::default() });
        deck.transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent::plain("Centered title"),
                    transform: Some(Transform::from_points(40.0, 40.0, 400.0, 60.0).unwrap()),
                    alt: None,
                    placeholder: None,
                },
            )?;
            edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "ellipse".to_string(),
                    transform: Some(Transform::from_points(60.0, 120.0, 120.0, 80.0).unwrap()),
                    fill: Some(Fill::Solid { color: "#22AA44".to_string() }),
                    line: Some(Line { color: Some("#000000".to_string()), width_emu: points_to_emu(1.0) }),
                    alt: None,
                },
            )?;
            edit.add_table(
                &slide,
                TableElementInput {
                    column_widths_emu: vec![points_to_emu(100.0).unwrap(), points_to_emu(100.0).unwrap()],
                    rows: vec![TableRow {
                        height_emu: points_to_emu(24.0).unwrap(),
                        cells: vec![TableCell::plain("Alpha"), TableCell::plain("Beta")],
                    }],
                    transform: Some(Transform::from_points(60.0, 220.0, 200.0, 24.0).unwrap()),
                    alt: None,
                },
            )?;
            let asset = edit.add_asset("pixel.png", "image/png", tiny_png(), None)?;
            edit.add_image(
                &slide,
                ImageElementInput {
                    asset,
                    transform: Some(Transform::from_points(300.0, 120.0, 80.0, 40.0).unwrap()),
                    alt: "Pixel".to_string(),
                },
            )?;
            edit.add_chart(
                &slide,
                ChartElementInput {
                    chart_type: ChartType::Column,
                    data: ChartData {
                        categories: vec!["Q1".to_string()],
                        series: vec![ChartSeries { name: "S".to_string(), values: vec![1.0] }],
                    },
                    transform: Some(Transform::from_points(300.0, 220.0, 200.0, 150.0).unwrap()),
                    alt: "Sales".to_string(),
                },
            )?;
            edit.add_formula(
                &slide,
                FormulaElementInput {
                    expression: "x^2".to_string(),
                    transform: Some(Transform::from_points(60.0, 320.0, 120.0, 40.0).unwrap()),
                    alt: None,
                },
            )?;
            let clip = edit.add_asset("clip.mp4", "video/mp4", b"not-a-real-video".to_vec(), None)?;
            edit.add_media(
                &slide,
                MediaKind::Video,
                clip,
                "Clip",
                Some(Transform::from_points(300.0, 320.0, 120.0, 60.0).unwrap()),
            )?;
            Ok(())
        })
        .expect("medium deck builds");
        deck
    }

    #[test]
    fn a_medium_deck_projects_every_supported_construct() {
        let export = export_pdf_bytes(&medium_deck()).expect("pdf builds");
        let text = String::from_utf8_lossy(&export.bytes);

        assert!(export.bytes.starts_with(b"%PDF-1.7"));
        assert_eq!(count(&export.bytes, b"/Type /Page "), 1, "one page per slide");
        assert_eq!(count(&export.bytes, b"/Subtype /Image"), 1, "the PNG asset is embedded");
        assert!(text.contains("/Im1 Do"), "the picture is painted");
        assert!(text.contains("(Centered title) Tj"), "text is painted");
        assert!(text.contains("(Alpha) Tj") && text.contains("(Beta) Tj"), "table cells are painted");
        assert!(text.contains(" c\n"), "the ellipse uses curves");
        assert!(text.contains("0.133 0.667 0.267 rg"), "the shape fill colour is painted");

        let codes = export.losses.iter().map(|loss| loss.code).collect::<Vec<_>>();
        assert!(!codes.contains(&LOSS_IMAGE), "the PNG picture embeds without loss");
        assert!(codes.contains(&LOSS_CHART), "charts are reported");
        assert!(codes.contains(&LOSS_FORMULA), "formulas are reported");
        assert!(codes.contains(&LOSS_MEDIA), "media is reported");
    }

    #[test]
    fn the_request_publishes_the_file_and_applies_the_loss_policy() {
        let deck = medium_deck();
        let dir = std::env::temp_dir().join(format!("open-ppt-pdf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let target = dir.join("deck.pdf");

        let report = export(&deck, &json!({ "target": target.display().to_string(), "overwrite": true, "lossPolicy": "warn" }))
            .expect("export succeeds with warnings");
        assert_eq!(report["format"], json!("pdf"));
        assert!(report["bytes"].as_u64().unwrap_or(0) > 0);
        assert!(report["losses"].as_array().expect("losses array").len() >= 3);
        assert_eq!(report["losses"][0]["severity"], json!("warning"));
        assert!(report["losses"][0]["slideId"].is_string());
        assert!(report["losses"][0]["elementId"].is_string());
        let bytes = fs::read(&target).expect("published pdf");
        assert!(bytes.starts_with(b"%PDF-1.7"));

        let refused = export(
            &deck,
            &json!({ "target": dir.join("refused.pdf").display().to_string(), "overwrite": true, "lossPolicy": "error" }),
        )
        .expect_err("a lossy deck is refused under the strict policy");
        assert_eq!(refused.code, "export/loss-exceeded");
        assert!(!dir.join("refused.pdf").exists(), "nothing is published when the policy refuses");

        let _ = fs::remove_dir_all(&dir);
    }
}
