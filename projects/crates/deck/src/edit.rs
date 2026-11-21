//! JSON edit scripts for the Node-API and CLI surfaces.
//!
//! One call applies one atomic transaction of typed operations. Arguments arrive
//! in authoring units (points, CSS colours, base64 bytes) and are mapped to the
//! upstream model here; deck semantics and revision handling stay upstream.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use document_presentation::authored::{
    AssetId, ChartData, ChartElementInput, Deck, DeckEdit, DeckError, Element, ElementContent, ElementId, Fill,
    FormulaElementInput, ImageElementInput, LayoutId, Line, LockFlags, MediaKind, ShapeElementInput, SlideAddOptions, SlideId,
    TableCell, TableElementInput, TableRow, TextContent, TextElementInput, Transform, codes,
};
use serde_json::{Value, json};

use super::{ServiceError, summary};

/// Applies one atomic edit script and returns the committed summary.
pub fn edit(deck: &mut Deck, request: &Value) -> Result<Value, ServiceError> {
    let expected = expected_revision(request)?;
    let operations = request
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| ServiceError::new("session/argument-invalid", "`operations` must be an array"))?;
    if operations.is_empty() {
        return Err(ServiceError::new("session/argument-invalid", "an edit script needs at least one operation"));
    }
    let (results, resolved, deferred) = deck
        .transaction(expected, |draft| {
            let mut state = EditState { resolved: HashMap::new(), results: Vec::new(), deferred: Vec::new() };
            for operation in operations {
                state.apply(draft, operation)?;
            }
            Ok((state.results, state.resolved, state.deferred))
        })
        .map_err(ServiceError::from)?;
    apply_deferred(deck, deferred)?;
    Ok(json!({ "deck": summary(deck), "revision": deck.revision(), "results": results, "resolved": resolved }))
}

/// One edit the upstream draft API has no setter for.
///
/// The draft is validated while the transaction runs; the document is then
/// cloned, changed and rebuilt so a failure in the closure still consumes no
/// revision.
enum Deferred {
    Reorder { slide: SlideId, element: ElementId, index: usize },
    Background { slide: SlideId, fill: Option<Fill> },
}

/// Applies deferred edits to the committed document and rebuilds the deck.
///
/// `Deck::from_document` starts a fresh history, so the revision the transaction
/// already committed is kept as is: one edit script is still one bump.
fn apply_deferred(deck: &mut Deck, deferred: Vec<Deferred>) -> Result<(), ServiceError> {
    if deferred.is_empty() {
        return Ok(());
    }
    let mut document = deck.document().clone();
    for operation in deferred {
        match operation {
            Deferred::Reorder { slide, element, index } => {
                let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide)
                else {
                    continue;
                };
                let Some(position) = model.elements.iter().position(|candidate| candidate.id == element)
                else {
                    continue;
                };
                let moved = model.elements.remove(position);
                model.elements.insert(index.min(model.elements.len()), moved);
            }
            Deferred::Background { slide, fill } => {
                if let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide) {
                    model.background = fill;
                }
            }
        }
    }
    *deck = Deck::from_document(document).map_err(ServiceError::from)?;
    Ok(())
}

/// Returns one slide with its elements in authoring units.
pub fn slide_detail(deck: &Deck, slide_id: &str) -> Result<Value, ServiceError> {
    let slide = deck
        .slides()
        .iter()
        .find(|slide| slide.id.as_str() == slide_id)
        .ok_or_else(|| ServiceError::new("object/not-found", format!("slide `{slide_id}` does not exist")))?;
    let index = deck.slides().iter().position(|candidate| candidate.id == slide.id).unwrap_or(0);
    let elements = slide
        .elements
        .iter()
        .map(|element| {
            let mut value = json!({
                "id": element.id.as_str(),
                "kind": element.kind.as_str(),
                "transform": transform_value(element.transform),
                "alt": element.alt,
                "locked": element.lock.as_ref().is_some_and(|flags| flags.move_ || flags.resize || flags.style || flags.remove),
                "content": element_content(element),
                "style": element_style(element),
            });
            if let Some(text) = element_text(element) {
                value["text"] = Value::String(text);
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "id": slide.id.as_str(),
        "index": index,
        "title": slide.title(),
        "hidden": slide.hidden,
        "layout": slide.layout.as_ref().map(|layout| layout.as_str()),
        "background": slide.background.as_ref().map(fill_value),
        "notes": slide.notes,
        "elements": elements,
    }))
}

/// One asset with its bytes, for preview surfaces that legitimately need pixels.
pub fn asset_bytes(deck: &Deck, asset_id: &str) -> Result<Value, ServiceError> {
    let asset = deck
        .document()
        .assets
        .iter()
        .find(|asset| asset.id.as_str() == asset_id)
        .ok_or_else(|| ServiceError::new("object/not-found", format!("asset `{asset_id}` does not exist")))?;
    Ok(json!({
        "id": asset.id.as_str(),
        "mediaType": asset.media_type,
        "base64": STANDARD.encode(&asset.bytes),
    }))
}

/// Fill as the CSS colour the renderers understand, or `null` for inheritance.
fn fill_value(fill: &Fill) -> Value {
    match fill {
        Fill::None => Value::String("none".to_string()),
        Fill::Solid { color } => Value::String(color.clone()),
        Fill::Inherit => Value::Null,
    }
}

fn line_value(line: &Line) -> Value {
    json!({ "color": line.color, "widthPt": line.width_emu.map(points) })
}

/// Renderable payload of one element, in authoring units.
fn element_content(element: &Element) -> Value {
    match &element.content {
        ElementContent::Text(_) => Value::Null,
        ElementContent::Image(image) => json!({ "assetId": image.asset.as_str() }),
        ElementContent::Shape(shape) => json!({
            "geometry": shape.geometry,
            "fill": shape.fill.as_ref().map(fill_value),
            "line": shape.line.as_ref().map(line_value),
        }),
        ElementContent::Table(table) => json!({
            "columnWidthsPt": table.column_widths_emu.iter().map(|emu| points(*emu)).collect::<Vec<_>>(),
            "rows": table
                .rows
                .iter()
                .map(|row| json!({
                    "heightPt": points(row.height_emu),
                    "cells": row
                        .cells
                        .iter()
                        .map(|cell| json!({
                            "text": cell.text.text(),
                            "fill": cell.fill.as_ref().map(fill_value),
                        }))
                        .collect::<Vec<_>>(),
                }))
                .collect::<Vec<_>>(),
        }),
        ElementContent::Chart(chart) => json!({ "chartType": chart.chart_type, "data": chart.data }),
        ElementContent::Formula(formula) => json!({ "expression": formula.expression }),
        ElementContent::Media(media) => json!({
            "assetId": media.asset.as_str(),
            "mediaType": match media.media_kind {
                MediaKind::Audio => "audio",
                MediaKind::Video => "video",
            },
        }),
    }
}

/// First-run text styling, so a canvas can draw text without rewriting the model.
fn element_style(element: &Element) -> Value {
    let (paragraphs, runs) = match &element.content {
        ElementContent::Text(text) => (Some(text), text.paragraphs.first().and_then(|paragraph| paragraph.runs.first())),
        _ => (None, None),
    };
    json!({
        "fill": element.style.fill.as_ref().map(fill_value),
        "line": element.style.line.as_ref().map(line_value),
        "fontFamily": element.style.font_family.clone(),
        "sizePt": runs.and_then(|run| run.size_pt),
        "bold": runs.and_then(|run| run.bold).unwrap_or(false),
        "italic": runs.and_then(|run| run.italic).unwrap_or(false),
        "color": runs.and_then(|run| run.color.clone()),
        "align": paragraphs.and_then(|text| text.paragraphs.first()).and_then(|paragraph| paragraph.align),
    })
}

/// Lists embedded assets with sizes and hashes, never their bytes.
pub fn assets(deck: &Deck) -> Value {
    let items = deck
        .document()
        .assets
        .iter()
        .map(|asset| {
            json!({
                "id": asset.id.as_str(),
                "name": asset.name,
                "mediaType": asset.media_type,
                "bytes": asset.bytes.len(),
                "sha256": asset.sha256,
                "license": asset.license,
            })
        })
        .collect::<Vec<_>>();
    json!({ "assets": items })
}

/// Restores the previous committed document; `changed` is false at the start.
pub fn undo(deck: &mut Deck) -> Value {
    let changed = deck.undo();
    json!({ "deck": summary(deck), "revision": deck.revision(), "changed": changed })
}

/// Reapplies the last undone document; `changed` is false when redo is empty.
pub fn redo(deck: &mut Deck) -> Value {
    let changed = deck.redo();
    json!({ "deck": summary(deck), "revision": deck.revision(), "changed": changed })
}

struct EditState {
    resolved: HashMap<String, String>,
    results: Vec<Option<String>>,
    deferred: Vec<Deferred>,
}

impl EditState {
    fn apply(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<(), DeckError> {
        let name = operation.get("op").and_then(Value::as_str).ok_or_else(|| invalid("every operation needs an `op` name"))?;
        let created = match name {
            "slide.add" => Some(self.add_slide(draft, operation)?),
            "slide.remove" => {
                draft.remove_slide(&self.slide(operation)?)?;
                None
            }
            "slide.reorder" => {
                let order = array(operation, "slideIds")?
                    .iter()
                    .map(|value| self.slide_id_value(value))
                    .collect::<Result<Vec<_>, _>>()?;
                draft.reorder_slides(&order)?;
                None
            }
            "slide.duplicate" => {
                let source = self.slide(operation)?;
                let created = draft.duplicate_slide(&source, optional_usize(operation, "index")?)?;
                Some(created.as_str().to_string())
            }
            "slide.setTitle" => {
                let slide = self.slide(operation)?;
                draft.set_slide_title(&slide, required_str(operation, "text")?)?;
                None
            }
            "slide.setNotes" => {
                let slide = self.slide(operation)?;
                draft.set_slide_notes(&slide, optional_string(operation, "text")?)?;
                None
            }
            "slide.setHidden" => {
                let slide = self.slide(operation)?;
                let hidden = operation
                    .get("hidden")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| invalid("`slide.setHidden` needs a boolean `hidden`"))?;
                draft.set_slide_hidden(&slide, hidden)?;
                None
            }
            "slide.setBackground" => {
                let slide = self.slide(operation)?;
                let fill = background_fill(required(operation, "fill")?)?;
                if draft.document().slide(&slide).is_none() {
                    return Err(
                        DeckError::new(codes::OBJECT_NOT_FOUND, format!("slide `{slide}` does not exist")).with_slide(slide)
                    );
                }
                self.deferred.push(Deferred::Background { slide, fill });
                None
            }
            "text.add" => {
                let slide = self.slide(operation)?;
                let created = draft.add_text(
                    &slide,
                    TextElementInput {
                        text: text_content(operation.get("text"))?,
                        transform: optional_transform(operation)?,
                        alt: optional_string(operation, "alt")?,
                        placeholder: placeholder(operation)?,
                    },
                )?;
                Some(created.as_str().to_string())
            }
            "image.add" => {
                let slide = self.slide(operation)?;
                let asset = self.asset(draft, operation)?;
                let created = draft.add_image(
                    &slide,
                    ImageElementInput {
                        asset,
                        transform: optional_transform(operation)?,
                        alt: required_str(operation, "alt")?.to_string(),
                    },
                )?;
                Some(created.as_str().to_string())
            }
            "shape.add" => {
                let slide = self.slide(operation)?;
                let created = draft.add_shape(
                    &slide,
                    ShapeElementInput {
                        geometry: required_str(operation, "geometry")?.to_string(),
                        transform: optional_transform(operation)?,
                        fill: optional_fill(operation.get("fill"))?,
                        line: optional_line(operation.get("line"))?,
                        alt: optional_string(operation, "alt")?,
                    },
                )?;
                Some(created.as_str().to_string())
            }
            "table.add" => {
                let slide = self.slide(operation)?;
                let created = draft.add_table(&slide, table_input(operation)?)?;
                Some(created.as_str().to_string())
            }
            "chart.add" => {
                let slide = self.slide(operation)?;
                let created = draft.add_chart(
                    &slide,
                    ChartElementInput {
                        chart_type: serde_json::from_value(required(operation, "chartType")?.clone())
                            .map_err(|error| invalid(format!("unusable `chartType`: {error}")))?,
                        data: chart_data(operation)?,
                        transform: optional_transform(operation)?,
                        alt: required_str(operation, "alt")?.to_string(),
                    },
                )?;
                Some(created.as_str().to_string())
            }
            "formula.add" => {
                let slide = self.slide(operation)?;
                let created = draft.add_formula(
                    &slide,
                    FormulaElementInput {
                        expression: required_str(operation, "expression")?.to_string(),
                        transform: optional_transform(operation)?,
                        alt: optional_string(operation, "alt")?,
                    },
                )?;
                Some(created.as_str().to_string())
            }
            "media.add" => {
                let slide = self.slide(operation)?;
                let media_kind = match required_str(operation, "kind")? {
                    "audio" => MediaKind::Audio,
                    "video" => MediaKind::Video,
                    other => return Err(invalid(format!("media kind must be `audio` or `video`, not `{other}`"))),
                };
                let asset = self.asset(draft, operation)?;
                let created = draft.add_media(
                    &slide,
                    media_kind,
                    asset,
                    required_str(operation, "alt")?,
                    optional_transform(operation)?,
                )?;
                Some(created.as_str().to_string())
            }
            "element.updateTransform" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                draft.update_transform(&slide, &element, required_transform(operation)?)?;
                None
            }
            "element.updateAlt" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                draft.update_alt(&slide, &element, optional_string(operation, "alt")?)?;
                None
            }
            "element.setText" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                draft.set_element_text(&slide, &element, text_content(operation.get("text"))?)?;
                None
            }
            "element.setChartData" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                draft.set_chart_data(&slide, &element, chart_data(operation)?)?;
                None
            }
            "element.lock" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                draft.lock_element(&slide, &element, lock_flags(operation.get("flags"))?)?;
                None
            }
            "element.reorder" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let model = draft.document().slide(&slide).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("slide `{slide}` does not exist")).with_slide(slide.clone())
                })?;
                if model.element(&element).is_none() {
                    return Err(DeckError::new(
                        codes::OBJECT_NOT_FOUND,
                        format!("element `{element}` does not exist on this slide"),
                    )
                    .with_slide(slide)
                    .with_element(element));
                }
                let index = required_usize(operation, "index")?;
                if index >= model.elements.len() {
                    return Err(invalid(format!("`index` must be below {} for this slide", model.elements.len())));
                }
                self.deferred.push(Deferred::Reorder { slide, element, index });
                None
            }
            "element.remove" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                draft.remove_element(&slide, &element)?;
                None
            }
            "asset.add" => Some(self.add_asset(draft, operation)?),
            other => return Err(DeckError::new(codes::OBJECT_UNSUPPORTED, format!("operation `{other}` is not editable yet"))),
        };
        if let Some(id) = &created
            && let Some(token) = operation.get("as").and_then(Value::as_str)
        {
            self.resolved.insert(token.to_string(), id.clone());
        }
        self.results.push(created);
        Ok(())
    }

    fn add_slide(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<String, DeckError> {
        let layout = operation.get("layout").and_then(Value::as_str).map(|value| self.resolve_value(value)).transpose()?;
        let layout = layout
            .map(|value| LayoutId::new(value).ok_or_else(|| invalid("slide layout identity must not be empty")))
            .transpose()?;
        let slide = draft.add_slide(SlideAddOptions { layout, index: optional_usize(operation, "index")? })?;
        Ok(slide.as_str().to_string())
    }

    fn add_asset(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<String, DeckError> {
        let name = required_str(operation, "name")?.to_string();
        let media_type = required_str(operation, "mediaType")?.to_string();
        let bytes = decode_bytes(operation.get("base64"))?;
        let asset = draft.add_asset(name, media_type, bytes, optional_string(operation, "license")?)?;
        Ok(asset.as_str().to_string())
    }

    fn asset(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<AssetId, DeckError> {
        if let Some(inline) = operation.get("asset") {
            let name = required_str(inline, "name")?.to_string();
            let media_type = required_str(inline, "mediaType")?.to_string();
            let bytes = decode_bytes(inline.get("base64"))?;
            return draft.add_asset(name, media_type, bytes, optional_string(inline, "license")?);
        }
        let value = self.resolve_value(required_str(operation, "assetId")?)?;
        AssetId::new(value).ok_or_else(|| invalid("asset identity must not be empty"))
    }

    fn slide(&self, operation: &Value) -> Result<SlideId, DeckError> {
        self.slide_id_value(required(operation, "slideId")?)
    }

    fn slide_id_value(&self, value: &Value) -> Result<SlideId, DeckError> {
        let raw = value.as_str().ok_or_else(|| invalid("slide identity must be a string"))?;
        SlideId::new(self.resolve_value(raw)?).ok_or_else(|| invalid("slide identity must not be empty"))
    }

    fn element(&self, operation: &Value) -> Result<ElementId, DeckError> {
        let raw = required_str(operation, "elementId")?;
        ElementId::new(self.resolve_value(raw)?).ok_or_else(|| invalid("element identity must not be empty"))
    }

    fn resolve_value(&self, raw: &str) -> Result<String, DeckError> {
        if !raw.starts_with("$tx:") {
            return Ok(raw.to_string());
        }
        self.resolved
            .get(raw)
            .cloned()
            .ok_or_else(|| invalid(format!("transaction reference `{raw}` was not created earlier in this script")))
    }
}

fn expected_revision(request: &Value) -> Result<Option<u64>, ServiceError> {
    match request.get("expectedRevision") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| ServiceError::new("session/argument-invalid", "`expectedRevision` must be a non-negative integer")),
    }
}

fn required<'a>(operation: &'a Value, key: &str) -> Result<&'a Value, DeckError> {
    operation.get(key).ok_or_else(|| invalid(format!("operation `{}` needs `{key}`", op_name(operation))))
}

fn required_str<'a>(operation: &'a Value, key: &str) -> Result<&'a str, DeckError> {
    required(operation, key)?.as_str().ok_or_else(|| invalid(format!("`{key}` must be a string")))
}

fn optional_string(operation: &Value, key: &str) -> Result<Option<String>, DeckError> {
    match operation.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(invalid(format!("`{key}` must be a string or null"))),
    }
}

fn optional_usize(operation: &Value, key: &str) -> Result<Option<usize>, DeckError> {
    match operation.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a non-negative integer"))),
    }
}

fn required_usize(operation: &Value, key: &str) -> Result<usize, DeckError> {
    required(operation, key)?
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| invalid(format!("`{key}` must be a non-negative integer")))
}

/// Parses `null` or `{ "kind": "solid", "color": "#RRGGBB" }`.
fn background_fill(value: &Value) -> Result<Option<Fill>, DeckError> {
    if value.is_null() {
        return Ok(None);
    }
    let kind = value.get("kind").and_then(Value::as_str).ok_or_else(|| invalid("`fill.kind` must be `solid`"))?;
    if kind != "solid" {
        return Err(invalid(format!("`fill.kind` must be `solid`, not `{kind}`")));
    }
    let color = value.get("color").and_then(Value::as_str).ok_or_else(|| invalid("a solid background needs a `color`"))?;
    let trimmed = color.trim().trim_start_matches('#');
    let expanded = match trimmed.len() {
        3 if trimmed.chars().all(|character| character.is_ascii_hexdigit()) => {
            trimmed.chars().flat_map(|character| [character, character]).collect::<String>()
        }
        6 if trimmed.chars().all(|character| character.is_ascii_hexdigit()) => trimmed.to_string(),
        _ => return Err(invalid("background `color` must be a 3- or 6-digit hex colour")),
    };
    Ok(Some(Fill::Solid { color: format!("#{}", expanded.to_ascii_uppercase()) }))
}

fn array<'a>(operation: &'a Value, key: &str) -> Result<&'a Vec<Value>, DeckError> {
    required(operation, key)?.as_array().ok_or_else(|| invalid(format!("`{key}` must be an array")))
}

fn op_name(operation: &Value) -> &str {
    operation.get("op").and_then(Value::as_str).unwrap_or("unknown")
}

fn invalid(message: impl Into<String>) -> DeckError {
    DeckError::new(codes::ARGUMENT_INVALID, message)
}

fn text_content(value: Option<&Value>) -> Result<TextContent, DeckError> {
    match value {
        Some(Value::String(text)) => Ok(TextContent::plain(text.clone())),
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| invalid(format!("unusable rich text: {error}"))),
        None => Err(invalid("text input is missing")),
    }
}

fn optional_transform(operation: &Value) -> Result<Option<Transform>, DeckError> {
    match operation.get("transform") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => required_transform(operation).map(Some),
    }
}

fn required_transform(operation: &Value) -> Result<Transform, DeckError> {
    let value = required(operation, "transform")?;
    let number = |key: &str| {
        value.get(key).and_then(Value::as_f64).ok_or_else(|| invalid(format!("`transform.{key}` must be a number in points")))
    };
    let mut transform = Transform::from_points(number("xPt")?, number("yPt")?, number("widthPt")?, number("heightPt")?)
        .ok_or_else(|| invalid("transform values must be finite point numbers"))?;
    transform.rotation_deg = value.get("rotationDeg").and_then(Value::as_f64);
    Ok(transform)
}

fn transform_value(transform: Transform) -> Value {
    json!({
        "xPt": points(transform.x_emu),
        "yPt": points(transform.y_emu),
        "widthPt": points(transform.width_emu),
        "heightPt": points(transform.height_emu),
        "rotationDeg": transform.rotation_deg,
    })
}

fn points(emu: i64) -> f64 {
    (emu as f64 / 12_700.0 * 10_000.0).round() / 10_000.0
}

fn optional_fill(value: Option<&Value>) -> Result<Option<Fill>, DeckError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(color)) => Ok(Some(Fill::Solid { color: color.clone() })),
        Some(_) => Err(invalid("`fill` must be a CSS colour string or null")),
    }
}

fn optional_line(value: Option<&Value>) -> Result<Option<Line>, DeckError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let width = value.get("widthPt").and_then(Value::as_f64);
            Ok(Some(Line {
                color: value.get("color").and_then(Value::as_str).map(str::to_string),
                width_emu: width.and_then(document_presentation::authored::points_to_emu),
            }))
        }
    }
}

fn table_input(operation: &Value) -> Result<TableElementInput, DeckError> {
    let rows = array(operation, "rows")?;
    let columns = rows.first().and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    if columns == 0 || rows.is_empty() {
        return Err(invalid("tables need at least one column and one row"));
    }
    let widths = array(operation, "columnWidthsPt")?;
    if widths.len() != columns {
        return Err(invalid("`columnWidthsPt` must list one positive width per column"));
    }
    let heights = array(operation, "rowHeightsPt")?;
    if heights.len() != rows.len() {
        return Err(invalid("`rowHeightsPt` must list one positive height per row"));
    }
    let column_widths_emu = widths
        .iter()
        .map(|value| {
            value
                .as_f64()
                .and_then(document_presentation::authored::points_to_emu)
                .ok_or_else(|| invalid("`columnWidthsPt` must use finite positive point widths"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut table_rows = Vec::with_capacity(rows.len());
    for (row, height) in rows.iter().zip(heights) {
        let cells = row.as_array().ok_or_else(|| invalid("each table row must be an array of cells"))?;
        if cells.len() != columns {
            return Err(invalid("table rows must share the same column count"));
        }
        let height_emu = height
            .as_f64()
            .and_then(document_presentation::authored::points_to_emu)
            .ok_or_else(|| invalid("`rowHeightsPt` must use finite positive point heights"))?;
        let cells = cells
            .iter()
            .map(|cell| {
                let text = text_content(cell.get("text"))?;
                let fill = optional_fill(cell.get("fill"))?;
                Ok(TableCell { text, fill, column_span: 1, row_span: 1 })
            })
            .collect::<Result<Vec<_>, DeckError>>()?;
        table_rows.push(TableRow { height_emu, cells });
    }
    Ok(TableElementInput {
        column_widths_emu,
        rows: table_rows,
        transform: optional_transform(operation)?,
        alt: optional_string(operation, "alt")?,
    })
}

fn chart_data(operation: &Value) -> Result<ChartData, DeckError> {
    serde_json::from_value(required(operation, "data")?.clone())
        .map_err(|error| invalid(format!("unusable chart data: {error}")))
}

fn lock_flags(value: Option<&Value>) -> Result<LockFlags, DeckError> {
    let value = value.ok_or_else(|| invalid("`element.lock` needs `flags`"))?;
    Ok(LockFlags {
        move_: value.get("move").and_then(Value::as_bool).unwrap_or(false),
        resize: value.get("resize").and_then(Value::as_bool).unwrap_or(false),
        style: value.get("style").and_then(Value::as_bool).unwrap_or(false),
        remove: value.get("remove").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn placeholder(operation: &Value) -> Result<Option<document_presentation::authored::PlaceholderRef>, DeckError> {
    match operation.get("placeholder") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let name = value.get("name").and_then(Value::as_str).ok_or_else(|| invalid("`placeholder.name` is required"))?;
            let layout = value
                .get("layoutId")
                .and_then(Value::as_str)
                .map(|id| LayoutId::new(id).ok_or_else(|| invalid("placeholder layout identity must not be empty")))
                .transpose()?;
            Ok(Some(document_presentation::authored::PlaceholderRef { layout, name: name.to_string() }))
        }
    }
}

fn decode_bytes(value: Option<&Value>) -> Result<Vec<u8>, DeckError> {
    let raw = value.and_then(Value::as_str).ok_or_else(|| invalid("inline assets need base64 `bytes`"))?;
    STANDARD.decode(raw).map_err(|error| invalid(format!("asset bytes are not valid base64: {error}")))
}

fn element_text(element: &document_presentation::authored::Element) -> Option<String> {
    match &element.content {
        document_presentation::authored::ElementContent::Text(text) => Some(text.text()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use document_presentation::authored::{
        DeckCreateOptions, ShapeElementInput, SlideAddOptions, TextContent, TextElementInput, Transform,
    };

    use super::*;

    fn two_shapes() -> (Deck, String, String, String) {
        let mut deck = Deck::create(DeckCreateOptions::default());
        let mut ids = (String::new(), String::new());
        let slide = deck
            .transaction(None, |edit| {
                let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
                let first = edit.add_shape(
                    &slide,
                    ShapeElementInput {
                        geometry: "rect".to_string(),
                        transform: Some(Transform::from_points(10.0, 10.0, 50.0, 50.0).unwrap()),
                        fill: Some(Fill::Solid { color: "#FF0000".to_string() }),
                        line: None,
                        alt: None,
                    },
                )?;
                let second = edit.add_shape(
                    &slide,
                    ShapeElementInput {
                        geometry: "rect".to_string(),
                        transform: Some(Transform::from_points(20.0, 20.0, 50.0, 50.0).unwrap()),
                        fill: Some(Fill::Solid { color: "#00FF00".to_string() }),
                        line: None,
                        alt: None,
                    },
                )?;
                ids = (first.as_str().to_string(), second.as_str().to_string());
                Ok(slide)
            })
            .expect("shape deck builds");
        (deck, slide.as_str().to_string(), ids.0, ids.1)
    }

    fn background_deck() -> (Deck, String) {
        let mut deck = Deck::create(DeckCreateOptions::default());
        let slide = deck
            .transaction(None, |edit| {
                let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
                edit.add_text(
                    &slide,
                    TextElementInput {
                        text: TextContent::plain("Background"),
                        transform: Some(Transform::from_points(40.0, 40.0, 400.0, 100.0).unwrap()),
                        alt: None,
                        placeholder: None,
                    },
                )?;
                Ok(slide)
            })
            .expect("text deck builds");
        (deck, slide.as_str().to_string())
    }

    fn svg(deck: &Deck) -> String {
        let report = crate::render::render(deck).expect("render succeeds");
        report["pages"][0]["svg"].as_str().expect("svg page").to_string()
    }

    #[test]
    fn element_reorder_changes_the_paint_order() {
        let (mut deck, slide, first, second) = two_shapes();
        let before = svg(&deck);
        assert!(
            before.find("fill=\"#FF0000\"").unwrap() < before.find("fill=\"#00FF00\"").unwrap(),
            "the first element paints first: {before}"
        );

        let revision = deck.revision();
        let report = edit(
            &mut deck,
            &json!({
                "operations": [{ "op": "element.reorder", "slideId": slide, "elementId": second, "index": 0 }],
            }),
        )
        .expect("reorder commits");

        assert_eq!(report["revision"], revision + 1, "one script is one revision bump");
        let after = svg(&deck);
        assert!(
            after.find("fill=\"#00FF00\"").unwrap() < after.find("fill=\"#FF0000\"").unwrap(),
            "the moved element paints first: {after}"
        );

        let detail = slide_detail(&deck, &slide).expect("slide exists");
        assert_eq!(detail["elements"][0]["id"], second);
        assert_eq!(detail["elements"][1]["id"], first);
    }

    #[test]
    fn slide_background_edit_reaches_render_and_exports() {
        let (mut deck, slide) = background_deck();
        let revision = deck.revision();
        let report = edit(
            &mut deck,
            &json!({
                "operations": [{
                    "op": "slide.setBackground",
                    "slideId": slide,
                    "fill": { "kind": "solid", "color": "112233" },
                }],
            }),
        )
        .expect("background commits");

        assert_eq!(report["revision"], revision + 1);
        assert_eq!(slide_detail(&deck, &slide).expect("slide")["background"], "#112233");
        assert!(svg(&deck).contains("fill=\"#112233\""), "SVG paints the new background");

        let directory = std::env::temp_dir().join("open-ppt-background-edit");
        std::fs::create_dir_all(&directory).expect("temp directory");
        let pdf = directory.join("background.pdf");
        crate::export::export(&deck, &json!({ "format": "pdf", "target": pdf.display().to_string(), "overwrite": true }))
            .expect("pdf export");
        let pdf_text = String::from_utf8_lossy(&std::fs::read(&pdf).expect("pdf bytes")).to_string();
        assert!(pdf_text.contains("0.07 0.13 0.2 rg"), "PDF paints the new background: {pdf_text}");

        let pptx = directory.join("background.pptx");
        crate::export::export(&deck, &json!({ "format": "pptx", "target": pptx.display().to_string(), "overwrite": true }))
            .expect("pptx export");
        let pptx_text = String::from_utf8_lossy(&std::fs::read(&pptx).expect("pptx bytes")).to_string();
        assert!(pptx_text.contains("112233"), "PPTX writes the new background");
    }

    #[test]
    fn clearing_a_slide_background_restores_the_default() {
        let (mut deck, slide) = background_deck();
        edit(
            &mut deck,
            &json!({
                "operations": [{
                    "op": "slide.setBackground",
                    "slideId": slide,
                    "fill": { "kind": "solid", "color": "#ABCDEF" },
                }],
            }),
        )
        .expect("background commits");
        assert!(svg(&deck).contains("fill=\"#ABCDEF\""));

        let revision = deck.revision();
        let report =
            edit(&mut deck, &json!({ "operations": [{ "op": "slide.setBackground", "slideId": slide, "fill": null }] }))
                .expect("background clears");

        assert_eq!(report["revision"], revision + 1, "clearing is a commit too");
        assert!(slide_detail(&deck, &slide).expect("slide")["background"].is_null());
        let cleared = svg(&deck);
        assert!(!cleared.contains("#ABCDEF"), "the old background is gone");
        assert!(cleared.contains("fill=\"#FFFFFF\""), "the default background takes over");
    }

    #[test]
    fn invalid_reorder_and_background_arguments_do_not_commit() {
        let (mut deck, slide, _, second) = two_shapes();
        let revision = deck.revision();

        let error = edit(
            &mut deck,
            &json!({
                "operations": [{ "op": "element.reorder", "slideId": slide, "elementId": "element-missing", "index": 0 }],
            }),
        )
        .expect_err("an unknown element is refused");
        assert_eq!(error.code, "object/not-found");

        let error = edit(
            &mut deck,
            &json!({
                "operations": [{ "op": "element.reorder", "slideId": slide, "elementId": second, "index": 9 }],
            }),
        )
        .expect_err("an out-of-range index is refused");
        assert_eq!(error.code, "session/argument-invalid");

        let error = edit(
            &mut deck,
            &json!({
                "operations": [{
                    "op": "slide.setBackground",
                    "slideId": slide,
                    "fill": { "kind": "gradient", "color": "#000000" },
                }],
            }),
        )
        .expect_err("an unknown fill kind is refused");
        assert_eq!(error.code, "session/argument-invalid");

        let error = edit(
            &mut deck,
            &json!({
                "operations": [{
                    "op": "slide.setBackground",
                    "slideId": slide,
                    "fill": { "kind": "solid", "color": "pink" },
                }],
            }),
        )
        .expect_err("an unusable colour is refused");
        assert_eq!(error.code, "session/argument-invalid");
        assert_eq!(deck.revision(), revision, "failed scripts consume no revision");

        let report = edit(
            &mut deck,
            &json!({
                "operations": [
                    { "op": "element.reorder", "slideId": slide, "elementId": second, "index": 0 },
                    { "op": "slide.setBackground", "slideId": slide, "fill": { "kind": "solid", "color": "#102030" } },
                ],
            }),
        )
        .expect("deferred operations commit together");
        assert_eq!(report["revision"], revision + 1, "two deferred operations still bump once");
    }
}
