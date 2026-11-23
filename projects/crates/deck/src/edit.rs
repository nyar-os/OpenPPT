//! JSON edit scripts for the Node-API and CLI surfaces.
//!
//! One call applies one atomic transaction of typed operations. Arguments arrive
//! in authoring units (points, CSS colours, base64 bytes) and are mapped to the
//! upstream model here; deck semantics and revision handling stay upstream.

use std::collections::{BTreeSet, HashMap};

use data_encoding::representation::Base64;

use crate::base64_text;
use document_presentation::authored::{
    AssetId, ChartData, ChartElementInput, Deck, DeckEdit, DeckError, DeckSize, EMU_PER_POINT, Element, ElementContent, ElementId,
    Fill, FormulaElementInput, ImageElementInput, LayoutId, Line, LockFlags, MediaKind, ShapeElementInput, SlideAddOptions, SlideId,
    TableCell, TableElementInput, TableRow, TextAlign, TextContent, TextElementInput, Transform, codes,
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
    MoveSlide { slide: SlideId, index: usize },
    Background { slide: SlideId, fill: Option<Fill> },
    TableRows { slide: SlideId, element: ElementId, rows: Vec<TableRow> },
    RemoveAsset { asset: AssetId },
    SetLayout { slide: SlideId, layout: Option<LayoutId> },
    SetFill { slide: SlideId, element: ElementId, fill: Option<Fill> },
    SetLine { slide: SlideId, element: ElementId, line: Option<Line> },
    SetColumnWidths { slide: SlideId, element: ElementId, widths: Vec<i64> },
    SetRowHeights { slide: SlideId, element: ElementId, heights: Vec<i64> },
    SetTitle { title: Option<String> },
    SetSize { size: DeckSize },
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
            Deferred::MoveSlide { slide, index } => {
                let Some(position) = document.slides.iter().position(|candidate| candidate.id == slide)
                else {
                    continue;
                };
                let moved = document.slides.remove(position);
                document.slides.insert(index.min(document.slides.len()), moved);
            }
            Deferred::Background { slide, fill } => {
                if let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide) {
                    model.background = fill;
                }
            }
            Deferred::TableRows { slide, element, rows } => {
                let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide)
                else {
                    continue;
                };
                let Some(target) = model.elements.iter_mut().find(|candidate| candidate.id == element)
                else {
                    continue;
                };
                if let ElementContent::Table(table) = &mut target.content {
                    table.rows = rows;
                }
            }
            Deferred::SetTitle { title } => document.title = title,
            Deferred::SetSize { size } => document.size = size,
            Deferred::RemoveAsset { asset } => document.assets.retain(|candidate| candidate.id != asset),
            Deferred::SetLayout { slide, layout } => {
                if let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide) {
                    model.layout = layout;
                }
            }
            Deferred::SetFill { slide, element, fill } => {
                let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide)
                else {
                    continue;
                };
                let Some(target) = model.elements.iter_mut().find(|candidate| candidate.id == element)
                else {
                    continue;
                };
                if let ElementContent::Shape(shape) = &mut target.content {
                    shape.fill = fill;
                }
            }
            Deferred::SetLine { slide, element, line } => {
                let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide)
                else {
                    continue;
                };
                let Some(target) = model.elements.iter_mut().find(|candidate| candidate.id == element)
                else {
                    continue;
                };
                if let ElementContent::Shape(shape) = &mut target.content {
                    shape.line = line;
                }
            }
            Deferred::SetColumnWidths { slide, element, widths } => {
                let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide)
                else {
                    continue;
                };
                let Some(target) = model.elements.iter_mut().find(|candidate| candidate.id == element)
                else {
                    continue;
                };
                if let ElementContent::Table(table) = &mut target.content {
                    table.column_widths_emu = widths;
                }
            }
            Deferred::SetRowHeights { slide, element, heights } => {
                let Some(model) = document.slides.iter_mut().find(|candidate| candidate.id == slide)
                else {
                    continue;
                };
                let Some(target) = model.elements.iter_mut().find(|candidate| candidate.id == element)
                else {
                    continue;
                };
                if let ElementContent::Table(table) = &mut target.content {
                    for (row, height) in table.rows.iter_mut().zip(heights) {
                        row.height_emu = height;
                    }
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
        "base64": base64_text(&asset.bytes),
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
                            "style": first_run_style(&cell.text),
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

/// First-run styling of one rich-text body, used for table cells.
fn first_run_style(text: &TextContent) -> Value {
    let paragraph = text.paragraphs.first();
    let run = paragraph.and_then(|paragraph| paragraph.runs.first());
    json!({
        "fontFamily": run.and_then(|run| run.font_family.clone()),
        "sizePt": run.and_then(|run| run.size_pt),
        "bold": run.and_then(|run| run.bold),
        "italic": run.and_then(|run| run.italic),
        "underline": run.and_then(|run| run.underline),
        "strike": run.and_then(|run| run.strike),
        "color": run.and_then(|run| run.color.clone()),
        "align": paragraph.and_then(|paragraph| paragraph.align),
        "hyperlink": run.and_then(|run| run.hyperlink.clone()),
    })
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
        "fontFamily": runs.and_then(|run| run.font_family.clone()).or_else(|| element.style.font_family.clone()),
        "sizePt": runs.and_then(|run| run.size_pt),
        "bold": runs.and_then(|run| run.bold).unwrap_or(false),
        "italic": runs.and_then(|run| run.italic).unwrap_or(false),
        "color": runs.and_then(|run| run.color.clone()),
        "align": paragraphs.and_then(|text| text.paragraphs.first()).and_then(|paragraph| paragraph.align),
        "hyperlink": runs.and_then(|run| run.hyperlink.clone()),
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
            "deck.replaceText" => Some(self.replace_text(draft, operation)?),
            "deck.setTitle" => {
                let title = match operation.get("title") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(text)) => {
                        let trimmed = text.trim();
                        if trimmed.is_empty() {
                            return Err(invalid("`deck.setTitle` needs a non-empty `title`, or null to clear it"));
                        }
                        Some(trimmed.to_owned())
                    }
                    Some(_) => return Err(invalid("`title` must be a string or null")),
                };
                self.deferred.push(Deferred::SetTitle { title });
                None
            }
            "deck.setSize" => {
                let size = if let Some(ratio) = operation.get("ratio") {
                    let name = ratio
                        .as_str()
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .ok_or_else(|| invalid("`ratio` must be a non-empty string such as `16:9`"))?;
                    DeckSize::named(name)
                        .ok_or_else(|| invalid(format!("unknown slide ratio `{name}`; use `16:9`, `4:3` or `16:10`")))?
                }
                else {
                    let width = operation.get("width").and_then(Value::as_f64);
                    let height = operation.get("height").and_then(Value::as_f64);
                    let (Some(width), Some(height)) = (width, height)
                    else {
                        return Err(invalid("`deck.setSize` needs `ratio`, or both `width` and `height` in points"));
                    };
                    DeckSize::from_points(width, height).ok_or_else(|| {
                        invalid("custom slide size must use finite positive point values")
                    })?
                };
                self.deferred.push(Deferred::SetSize { size });
                None
            }
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
            "slide.move" => {
                let slide = self.slide(operation)?;
                let index = required_usize(operation, "index")?;
                if draft.document().slide(&slide).is_none() {
                    return Err(
                        DeckError::new(codes::OBJECT_NOT_FOUND, format!("slide `{slide}` does not exist")).with_slide(slide)
                    );
                }
                let slides = draft.document().slides.len();
                if index >= slides {
                    return Err(invalid(format!("`index` must be below {slides} for this deck")));
                }
                self.deferred.push(Deferred::MoveSlide { slide, index });
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
            "slide.setLayout" => {
                let slide = self.slide(operation)?;
                if draft.document().slide(&slide).is_none() {
                    return Err(missing_slide(&slide));
                }
                let layout = match required(operation, "layout")? {
                    Value::Null => None,
                    Value::String(raw) => {
                        let resolved = self.resolve_value(raw)?;
                        let layout = LayoutId::new(resolved).ok_or_else(|| invalid("layout identity must not be empty"))?;
                        if !draft.document().layouts.iter().any(|candidate| candidate.id == layout) {
                            return Err(DeckError::new(codes::OBJECT_NOT_FOUND, format!("layout `{layout}` does not exist")));
                        }
                        Some(layout)
                    }
                    _ => return Err(invalid("`layout` must be a layout id string or null")),
                };
                self.deferred.push(Deferred::SetLayout { slide, layout });
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
            "element.duplicate" => Some(self.duplicate_element(draft, operation)?),
            "element.align" => {
                let slide = self.slide(operation)?;
                let elements = self.element_list(operation, "elementIds", 1)?;
                let edge = parse_align_edge(required_str(operation, "edge")?)?;
                let relative_to = parse_relative_to(operation)?;
                align_elements(draft, &slide, &elements, edge, relative_to)?;
                None
            }
            "element.distribute" => {
                let slide = self.slide(operation)?;
                let elements = self.element_list(operation, "elementIds", 3)?;
                let axis = parse_distribute_axis(required_str(operation, "axis")?)?;
                distribute_elements(draft, &slide, &elements, axis)?;
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
            "element.setTextStyle" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let style = TextStyle::parse(operation, "element.setTextStyle")?;
                let content = styled_content(draft, &slide, &element, &style)?;
                draft.set_element_text(&slide, &element, content)?;
                None
            }
            "element.setHyperlink" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let link = match operation.get("url") {
                    Some(Value::Null) => None,
                    Some(Value::String(url)) if !url.trim().is_empty() => Some(url.trim().to_owned()),
                    Some(Value::String(_)) => return Err(invalid("`url` must be a non-empty string or null")),
                    _ => return Err(invalid("`element.setHyperlink` needs a `url` string or null")),
                };
                let content = hyperlink_content(draft, &slide, &element, link)?;
                draft.set_element_text(&slide, &element, content)?;
                None
            }
            "element.setFill" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let fill = match required(operation, "fill")? {
                    Value::Null => None,
                    Value::String(color) => Some(Fill::Solid { color: color.clone() }),
                    _ => return Err(invalid("`fill` must be a CSS colour string or null")),
                };
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                if !matches!(target.content, ElementContent::Shape(_)) {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a shape; only shapes carry a fill"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                }
                self.deferred.push(Deferred::SetFill { slide, element, fill });
                None
            }
            "element.setLine" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let line = required_line(operation)?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                if !matches!(target.content, ElementContent::Shape(_)) {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a shape; only shapes carry a line"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                }
                self.deferred.push(Deferred::SetLine { slide, element, line });
                None
            }
            "table.setCell" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let row = required_usize(operation, "row")?;
                let column = required_usize(operation, "column")?;
                let text = operation.get("text").and_then(Value::as_str).map(str::to_string);
                let fill = match operation.get("fill") {
                    None => None,
                    Some(Value::Null) => Some(None),
                    Some(Value::String(color)) => Some(Some(Fill::Solid { color: color.clone() })),
                    Some(_) => return Err(invalid("`fill` must be a CSS colour string or null")),
                };
                if text.is_none() && fill.is_none() {
                    return Err(invalid("`table.setCell` needs `text` and/or `fill`"));
                }
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry cells"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                if row >= table.rows.len() {
                    return Err(invalid(format!("`row` {row} is outside the table's {} rows", table.rows.len())));
                }
                let columns = table.rows[row].cells.len();
                if column >= columns {
                    return Err(invalid(format!("`column` {column} is outside row {row}'s {columns} cells")));
                }
                let mut rows = queued_rows(&self.deferred, &slide, &element).unwrap_or_else(|| table.rows.clone());
                if let Some(text) = text {
                    rows[row].cells[column].text = TextContent::plain(text);
                }
                if let Some(fill) = fill {
                    rows[row].cells[column].fill = fill;
                }
                self.queue_table_rows(slide, element, rows);
                None
            }
            "table.setCellStyle" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let row = required_usize(operation, "row")?;
                let column = required_usize(operation, "column")?;
                let style = TextStyle::parse(operation, "table.setCellStyle")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry cells"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                if row >= table.rows.len() {
                    return Err(invalid(format!("`row` {row} is outside the table's {} rows", table.rows.len())));
                }
                let columns = table.rows[row].cells.len();
                if column >= columns {
                    return Err(invalid(format!("`column` {column} is outside row {row}'s {columns} cells")));
                }
                let mut rows = queued_rows(&self.deferred, &slide, &element).unwrap_or_else(|| table.rows.clone());
                style.apply(&mut rows[row].cells[column].text);
                self.queue_table_rows(slide, element, rows);
                None
            }
            "table.insertRow" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let index = required_usize(operation, "row")?;
                let requested = optional_string_array(operation, "cells")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry rows"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                let columns = table.column_widths_emu.len();
                let mut rows = queued_rows(&self.deferred, &slide, &element).unwrap_or_else(|| table.rows.clone());
                if index > rows.len() {
                    return Err(invalid(format!(
                        "`row` {index} is outside the table's insertion points 0..={}",
                        rows.len()
                    )));
                }
                if rows.iter().enumerate().any(|(position, row)| {
                    position < index && row.cells.iter().any(|cell| position + cell.row_span as usize > index)
                }) {
                    return Err(invalid("a merged cell spans across the insertion point; insert after it instead"));
                }
                let cells = match requested {
                    Some(texts) => {
                        if texts.len() != columns {
                            return Err(invalid(format!(
                                "`cells` lists {} entries but the table has {columns} columns",
                                texts.len()
                            )));
                        }
                        texts.into_iter().map(TableCell::plain).collect::<Vec<_>>()
                    }
                    None => (0..columns).map(|_| TableCell::plain("")).collect::<Vec<_>>(),
                };
                let height_emu = match operation.get("heightPt") {
                    None | Some(Value::Null) => rows
                        .get(index)
                        .or_else(|| rows.last())
                        .map(|row| row.height_emu)
                        .unwrap_or_else(|| document_presentation::authored::points_to_emu(24.0).unwrap_or_default()),
                    Some(raw) => raw
                        .as_f64()
                        .and_then(document_presentation::authored::points_to_emu)
                        .ok_or_else(|| invalid("`heightPt` must be a finite positive point height"))?,
                };
                rows.insert(index, TableRow { height_emu, cells });
                self.queue_table_rows(slide, element, rows);
                None
            }
            "table.deleteRow" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let index = required_usize(operation, "row")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry rows"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                let mut rows = queued_rows(&self.deferred, &slide, &element).unwrap_or_else(|| table.rows.clone());
                if index >= rows.len() {
                    return Err(invalid(format!("`row` {index} is outside the table's {} rows", rows.len())));
                }
                if rows.len() == 1 {
                    return Err(invalid("a table must keep at least one row"));
                }
                if rows.iter().enumerate().any(|(position, row)| {
                    row.cells.iter().any(|cell| {
                        (position < index && position + cell.row_span as usize > index)
                            || (position == index && cell.row_span > 1)
                    })
                }) {
                    return Err(invalid("row is covered by a merged cell; remove the merge before deleting it"));
                }
                rows.remove(index);
                self.queue_table_rows(slide, element, rows);
                None
            }
            "table.moveRow" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let from = required_usize(operation, "row")?;
                let to = required_usize(operation, "index")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry rows"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                let mut rows = queued_rows(&self.deferred, &slide, &element).unwrap_or_else(|| table.rows.clone());
                if from >= rows.len() {
                    return Err(invalid(format!("`row` {from} is outside the table's {} rows", rows.len())));
                }
                if to >= rows.len() {
                    return Err(invalid(format!("`index` {to} is outside the table's {} rows", rows.len())));
                }
                if rows.iter().any(|row| row.cells.iter().any(|cell| cell.row_span > 1)) {
                    return Err(invalid("row moves need a table without vertical merges"));
                }
                let row = rows.remove(from);
                rows.insert(to, row);
                self.queue_table_rows(slide, element, rows);
                None
            }
            "table.insertColumn" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let index = required_usize(operation, "column")?;
                let requested = optional_string_array(operation, "cells")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry columns"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                let mut widths = Self::queued_widths(&self.deferred, &slide, &element).unwrap_or_else(|| table.column_widths_emu.clone());
                let mut rows = queued_rows(&self.deferred, &slide, &element).unwrap_or_else(|| table.rows.clone());
                if index > widths.len() {
                    return Err(invalid(format!(
                        "`column` {index} is outside the table's insertion points 0..={}",
                        widths.len()
                    )));
                }
                for row in &rows {
                    let mut position = 0usize;
                    for cell in &row.cells {
                        if position < index && position + cell.column_span as usize > index {
                            return Err(invalid("a merged cell spans across the insertion point; insert after it instead"));
                        }
                        position += cell.column_span as usize;
                    }
                }
                let texts = match requested {
                    Some(texts) => {
                        if texts.len() != rows.len() {
                            return Err(invalid(format!(
                                "`cells` lists {} entries but the table has {} rows",
                                texts.len(),
                                rows.len()
                            )));
                        }
                        texts
                    }
                    None => vec![String::new(); rows.len()],
                };
                let width_emu = match operation.get("widthPt") {
                    None | Some(Value::Null) => {
                        widths.get(index).or_else(|| widths.last()).copied().unwrap_or_default()
                    }
                    Some(raw) => raw
                        .as_f64()
                        .and_then(document_presentation::authored::points_to_emu)
                        .ok_or_else(|| invalid("`widthPt` must be a finite positive point width"))?,
                };
                for (position_in_rows, row) in rows.iter_mut().enumerate() {
                    let text = texts[position_in_rows].clone();
                    let mut position = 0usize;
                    let mut cell_index = 0usize;
                    while position < index && cell_index < row.cells.len() {
                        position += row.cells[cell_index].column_span as usize;
                        cell_index += 1;
                    }
                    row.cells.insert(cell_index, TableCell::plain(text));
                }
                widths.insert(index, width_emu);
                self.queue_table_rows(slide.clone(), element.clone(), rows);
                self.deferred.push(Deferred::SetColumnWidths { slide, element, widths });
                None
            }
            "table.deleteColumn" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let index = required_usize(operation, "column")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry columns"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                let mut widths = Self::queued_widths(&self.deferred, &slide, &element).unwrap_or_else(|| table.column_widths_emu.clone());
                let mut rows = queued_rows(&self.deferred, &slide, &element).unwrap_or_else(|| table.rows.clone());
                if index >= widths.len() {
                    return Err(invalid(format!("`column` {index} is outside the table's {} columns", widths.len())));
                }
                if widths.len() == 1 {
                    return Err(invalid("a table must keep at least one column"));
                }
                for row in &rows {
                    let mut position = 0usize;
                    for cell in &row.cells {
                        let span = cell.column_span as usize;
                        if span > 1 && position <= index && position + span > index {
                            return Err(invalid("column is covered by a merged cell; remove the merge before deleting it"));
                        }
                        position += span;
                    }
                }
                for row in &mut rows {
                    let mut position = 0usize;
                    for cell_index in 0..row.cells.len() {
                        if position == index {
                            row.cells.remove(cell_index);
                            break;
                        }
                        position += row.cells[cell_index].column_span as usize;
                    }
                }
                widths.remove(index);
                self.queue_table_rows(slide.clone(), element.clone(), rows);
                self.deferred.push(Deferred::SetColumnWidths { slide, element, widths });
                None
            }
            "table.setColumnWidths" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let widths = point_list(operation, "columnWidthsPt")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry columns"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                if widths.len() != table.column_widths_emu.len() {
                    return Err(invalid(format!(
                        "`columnWidthsPt` needs one width per column; the table has {}",
                        table.column_widths_emu.len()
                    )));
                }
                self.deferred.push(Deferred::SetColumnWidths { slide, element, widths });
                None
            }
            "table.setRowHeights" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let heights = point_list(operation, "rowHeightsPt")?;
                let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
                let target = model.element(&element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let ElementContent::Table(table) = &target.content
                else {
                    return Err(DeckError::new(
                        codes::OBJECT_TYPE_MISMATCH,
                        format!("element `{element}` is not a table; only tables carry rows"),
                    )
                    .with_slide(slide.clone())
                    .with_element(element.clone()));
                };
                if heights.len() != table.rows.len() {
                    return Err(invalid(format!(
                        "`rowHeightsPt` needs one height per row; the table has {}",
                        table.rows.len()
                    )));
                }
                self.deferred.push(Deferred::SetRowHeights { slide, element, heights });
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
            "element.bringToFront" | "element.sendToBack" | "element.bringForward" | "element.sendBackward" => {
                let slide = self.slide(operation)?;
                let element = self.element(operation)?;
                let model = draft.document().slide(&slide).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("slide `{slide}` does not exist")).with_slide(slide.clone())
                })?;
                let position = model.elements.iter().position(|candidate| candidate.id == element).ok_or_else(|| {
                    DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                        .with_slide(slide.clone())
                        .with_element(element.clone())
                })?;
                let last = model.elements.len().saturating_sub(1);
                let index = match name {
                    "element.bringToFront" => last,
                    "element.sendToBack" => 0,
                    "element.bringForward" => (position + 1).min(last),
                    _ => position.saturating_sub(1),
                };
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
            "asset.remove" => {
                self.remove_asset(draft, operation)?;
                None
            }
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

    /// The most recently queued column widths for one table, if any.
fn queued_widths(deferred: &[Deferred], slide: &SlideId, element: &ElementId) -> Option<Vec<i64>> {
    deferred.iter().rev().find_map(|entry| match entry {
        Deferred::SetColumnWidths { slide: queued_slide, element: queued_element, widths }
            if queued_slide == slide && queued_element == element =>
        {
            Some(widths.clone())
        }
        _ => None,
    })
}

/// Queues a table's rebuilt rows, replacing an earlier cell edit on the same
    /// element so several `table.setCell` operations compose in one script.
    fn queue_table_rows(&mut self, slide: SlideId, element: ElementId, rows: Vec<TableRow>) {
        for entry in &mut self.deferred {
            if let Deferred::TableRows { slide: queued_slide, element: queued_element, rows: queued_rows } = entry
                && *queued_slide == slide
                && *queued_element == element
            {
                *queued_rows = rows;
                return;
            }
        }
        self.deferred.push(Deferred::TableRows { slide, element, rows });
    }

    fn add_slide(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<String, DeckError> {
        let layout = operation.get("layout").and_then(Value::as_str).map(|value| self.resolve_value(value)).transpose()?;
        let layout = layout
            .map(|value| LayoutId::new(value).ok_or_else(|| invalid("slide layout identity must not be empty")))
            .transpose()?;
        let title = optional_string(operation, "title")?;
        if title.as_deref().is_some_and(|value| value.trim().is_empty()) {
            return Err(invalid("`title` must not be blank"));
        }
        let notes = optional_string(operation, "notes")?;
        if notes.as_deref().is_some_and(|value| value.trim().is_empty()) {
            return Err(invalid("`notes` must not be blank"));
        }
        let slide = draft.add_slide(SlideAddOptions { layout, index: optional_usize(operation, "index")? })?;
        if let Some(title) = title {
            draft.set_slide_title(&slide, title.trim())?;
        }
        if let Some(notes) = notes {
            draft.set_slide_notes(&slide, Some(notes.trim().to_owned()))?;
        }
        Ok(slide.as_str().to_string())
    }

    /// Replaces every match of `find` in the deck's text elements and table cells.
    ///
    /// Text elements are rewritten through `set_element_text` inside the
    /// transaction; table cells have no upstream setter, so their rebuilt rows
    /// join the deferred list. Matching mirrors `deck.search`: case-insensitive
    /// by default, with an optional whole-word boundary. Replacement happens per
    /// run, so a match spanning two runs stays untouched. `slideIds` restricts
    /// the rewrite to those slides and unknown ids are ignored, exactly like the
    /// search filter. The result is the number of replaced matches.
    fn replace_text(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<String, DeckError> {
        let find = required_str(operation, "find")?;
        if find.is_empty() {
            return Err(invalid("`find` must be a non-empty string"));
        }
        let replace = required_str(operation, "replace")?;
        let case_sensitive = optional_bool(operation, "caseSensitive")?.unwrap_or(false);
        let whole_word = optional_bool(operation, "wholeWord")?.unwrap_or(false);
        let only = match operation.get("slideIds") {
            None | Some(Value::Null) => None,
            Some(value) => {
                let values = value.as_array().ok_or_else(|| invalid("`slideIds` must be an array"))?;
                let mut ids = BTreeSet::new();
                for item in values {
                    let raw = item.as_str().ok_or_else(|| invalid("`slideIds` must list slide ids"))?;
                    ids.insert(self.resolve_value(raw)?);
                }
                Some(ids)
            }
        };
        let mut replacements = 0_usize;
        let mut texts = Vec::new();
        let mut tables = Vec::new();
        for model in draft.document().slides.iter() {
            if only.as_ref().is_some_and(|ids| !ids.contains(model.id.as_str())) {
                continue;
            }
            for element in model.elements.iter() {
                match &element.content {
                    ElementContent::Text(text) => {
                        let (next, hits) = replace_in_text(text, find, replace, case_sensitive, whole_word);
                        if hits > 0 {
                            replacements += hits;
                            texts.push((model.id.clone(), element.id.clone(), next));
                        }
                    }
                    ElementContent::Table(table) => {
                        let (rows, hits) = replace_in_rows(&table.rows, find, replace, case_sensitive, whole_word);
                        if hits > 0 {
                            replacements += hits;
                            tables.push((model.id.clone(), element.id.clone(), rows));
                        }
                    }
                    _ => {}
                }
            }
        }
        for (slide, element, text) in texts {
            draft.set_element_text(&slide, &element, text)?;
        }
        for (slide, element, rows) in tables {
            self.deferred.push(Deferred::TableRows { slide, element, rows });
        }
        Ok(replacements.to_string())
    }

    fn add_asset(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<String, DeckError> {
        let name = required_str(operation, "name")?.to_string();
        let media_type = required_str(operation, "mediaType")?.to_string();
        let bytes = decode_bytes(operation.get("base64"))?;
        let asset = draft.add_asset(name, media_type, bytes, optional_string(operation, "license")?)?;
        Ok(asset.as_str().to_string())
    }

    /// Drops an embedded asset that no element references any more.
    ///
    /// The draft checks existence and usage; the removal itself joins the
    /// deferred list because the upstream editor has no asset setter.
    fn remove_asset(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<(), DeckError> {
        let value = self.resolve_value(required_str(operation, "assetId")?)?;
        let asset = AssetId::new(value).ok_or_else(|| invalid("asset identity must not be empty"))?;
        let document = draft.document();
        if !document.assets.iter().any(|candidate| candidate.id == asset) {
            return Err(DeckError::new(codes::OBJECT_NOT_FOUND, format!("asset `{asset}` does not exist")));
        }
        let users = document
            .slides
            .iter()
            .flat_map(|model| model.elements.iter())
            .filter(|element| match &element.content {
                ElementContent::Image(image) => image.asset == asset,
                ElementContent::Media(media) => media.asset == asset,
                _ => false,
            })
            .count();
        if users > 0 {
            return Err(DeckError::new(
                codes::OBJECT_UNSUPPORTED,
                format!("asset `{asset}` is still used by {users} element(s); remove them first"),
            ));
        }
        self.deferred.push(Deferred::RemoveAsset { asset });
        Ok(())
    }

    /// Rebuilds the source content as a new element at the end of the slide.
    ///
    /// `targetSlideId` may name another slide; the rebuilt element keeps the
    /// source transform plus any offset. Only content the public `add_*` inputs
    /// can express is duplicated, so an element that would lose crop,
    /// placeholder or alt data is refused instead of being copied partially.
    fn duplicate_element(&mut self, draft: &mut DeckEdit, operation: &Value) -> Result<String, DeckError> {
        let slide = self.slide(operation)?;
        let element = self.element(operation)?;
        let target = match operation.get("targetSlideId") {
            Some(value) => self.slide_id_value(value)?,
            None => slide.clone(),
        };
        if draft.document().slide(&target).is_none() {
            return Err(missing_slide(&target));
        }
        let source = {
            let model = draft.document().slide(&slide).ok_or_else(|| missing_slide(&slide))?;
            model.element(&element).cloned().ok_or_else(|| {
                DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{element}` does not exist on this slide"))
                    .with_slide(slide.clone())
                    .with_element(element.clone())
            })?
        };
        let mut transform = source.transform;
        transform.x_emu = transform.x_emu.saturating_add(offset_emu(operation, "offsetXPt")?);
        transform.y_emu = transform.y_emu.saturating_add(offset_emu(operation, "offsetYPt")?);
        let created = match &source.content {
            ElementContent::Text(text) => {
                if source.placeholder.is_some() {
                    return Err(unsupported_element(
                        &slide,
                        &element,
                        "placeholder-bound text cannot be duplicated without sharing one layout placeholder",
                    ));
                }
                draft.add_text(
                    &target,
                    TextElementInput {
                        text: text.clone(),
                        transform: Some(transform),
                        alt: source.alt.clone(),
                        placeholder: None,
                    },
                )?
            }
            ElementContent::Image(image) => {
                if image.crop.is_some() {
                    return Err(unsupported_element(&slide, &element, "cropped images cannot be rebuilt through `image.add`"));
                }
                let alt = source.alt.clone().filter(|alt| !alt.trim().is_empty()).ok_or_else(|| {
                    unsupported_element(
                        &slide,
                        &element,
                        "images without alternative text cannot be rebuilt through `image.add`",
                    )
                })?;
                draft.add_image(&target, ImageElementInput { asset: image.asset.clone(), transform: Some(transform), alt })?
            }
            ElementContent::Shape(shape) => draft.add_shape(
                &target,
                ShapeElementInput {
                    geometry: shape.geometry.clone(),
                    transform: Some(transform),
                    fill: shape.fill.clone(),
                    line: shape.line.clone(),
                    alt: source.alt.clone(),
                },
            )?,
            ElementContent::Table(table) => draft.add_table(
                &target,
                TableElementInput {
                    column_widths_emu: table.column_widths_emu.clone(),
                    rows: table.rows.clone(),
                    transform: Some(transform),
                    alt: source.alt.clone(),
                },
            )?,
            ElementContent::Chart(_) => {
                return Err(unsupported_element(
                    &slide,
                    &element,
                    "charts cannot be rebuilt through the public authoring inputs",
                ));
            }
            ElementContent::Formula(_) => {
                return Err(unsupported_element(
                    &slide,
                    &element,
                    "formulas cannot be rebuilt through the public authoring inputs",
                ));
            }
            ElementContent::Media(_) => {
                return Err(unsupported_element(
                    &slide,
                    &element,
                    "media cannot be rebuilt through the public authoring inputs",
                ));
            }
        };
        Ok(created.as_str().to_string())
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

    /// Reads a non-empty, duplicate-free element list and resolves transaction tokens.
    fn element_list(&self, operation: &Value, key: &str, minimum: usize) -> Result<Vec<ElementId>, DeckError> {
        let values = array(operation, key)?;
        if values.len() < minimum {
            return Err(invalid(format!("`{key}` needs at least {minimum} element ids")));
        }
        let mut seen = BTreeSet::new();
        let mut elements = Vec::with_capacity(values.len());
        for value in values {
            let raw = value.as_str().ok_or_else(|| invalid(format!("`{key}` must list element ids")))?;
            let resolved = self.resolve_value(raw)?;
            if !seen.insert(resolved.clone()) {
                return Err(invalid(format!("element `{resolved}` appears twice in `{key}`")));
            }
            let id = ElementId::new(resolved).ok_or_else(|| invalid("element identity must not be empty"))?;
            elements.push(id);
        }
        Ok(elements)
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

/// Parses the paragraph alignment shared by style operations.
fn optional_text_align(operation: &Value) -> Result<Option<TextAlign>, DeckError> {
    optional_string(operation, "align")?
        .map(|value| match value.as_str() {
            "left" => Ok(TextAlign::Left),
            "center" => Ok(TextAlign::Center),
            "right" => Ok(TextAlign::Right),
            "justify" => Ok(TextAlign::Justify),
            other => Err(invalid(format!("`align` must be `left`, `center`, `right` or `justify`, not `{other}`"))),
        })
        .transpose()
}

fn optional_string_array(operation: &Value, key: &str) -> Result<Option<Vec<String>>, DeckError> {
    match operation.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let entries = value.as_array().ok_or_else(|| invalid(format!("`{key}` must be an array of strings")))?;
            entries
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("`{key}` must be an array of strings")))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
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

/// Reads an optional point offset, keeping its sign and accepting zero.
fn offset_emu(operation: &Value, key: &str) -> Result<i64, DeckError> {
    let Some(value) = operation.get(key)
    else {
        return Ok(0);
    };
    if value.is_null() {
        return Ok(0);
    }
    let points = value
        .as_f64()
        .filter(|points| points.is_finite())
        .ok_or_else(|| invalid(format!("`{key}` must be a finite number in points")))?;
    let magnitude = (points.abs() * EMU_PER_POINT as f64).round();
    if magnitude > i64::MAX as f64 {
        return Err(invalid(format!("`{key}` is outside the supported range")));
    }
    let emu = magnitude as i64;
    Ok(if points < 0.0 { -emu } else { emu })
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

/// Reads a non-empty list of positive point values and converts them to EMU.
fn point_list(operation: &Value, key: &str) -> Result<Vec<i64>, DeckError> {
    let values = array(operation, key)?;
    if values.is_empty() {
        return Err(invalid(format!("`{key}` must list at least one point value")));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .and_then(document_presentation::authored::points_to_emu)
                .ok_or_else(|| invalid(format!("`{key}` must use finite positive point values")))
        })
        .collect()
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

/// Rebuilds rich text with every match of `find` replaced inside its runs.
fn replace_in_text(
    text: &TextContent,
    find: &str,
    replace: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> (TextContent, usize) {
    let mut text = text.clone();
    let mut hits = 0_usize;
    for paragraph in &mut text.paragraphs {
        for run in &mut paragraph.runs {
            let (next, count) = replace_matches(&run.text, find, replace, case_sensitive, whole_word);
            if count > 0 {
                run.text = next;
                hits += count;
            }
        }
    }
    (text, hits)
}

/// Rebuilds table rows with every match of `find` replaced inside the cell texts.
fn replace_in_rows(
    rows: &[TableRow],
    find: &str,
    replace: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> (Vec<TableRow>, usize) {
    let mut rows = rows.to_vec();
    let mut hits = 0_usize;
    for row in &mut rows {
        for cell in &mut row.cells {
            let (next, count) = replace_in_text(&cell.text, find, replace, case_sensitive, whole_word);
            if count > 0 {
                cell.text = next;
                hits += count;
            }
        }
    }
    (rows, hits)
}

/// Replaces non-overlapping matches of `find` in one run of text.
///
/// Matching mirrors the search options: case folding compares characters, and
/// `whole_word` refuses matches that touch a letter, digit or underscore.
fn replace_matches(text: &str, find: &str, replace: &str, case_sensitive: bool, whole_word: bool) -> (String, usize) {
    let needle = find.chars().collect::<Vec<_>>();
    if text.is_empty() || needle.is_empty() {
        return (text.to_string(), 0);
    }
    let haystack = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    let mut hits = 0_usize;
    while cursor < haystack.len() {
        let matched = matches_at(&haystack, cursor, &needle, case_sensitive)
            && !(whole_word && touches_word_char(&haystack, cursor, needle.len()));
        if matched {
            output.push_str(replace);
            cursor += needle.len();
            hits += 1;
        }
        else {
            output.push(haystack[cursor]);
            cursor += 1;
        }
    }
    (output, hits)
}

/// Whether `needle` starts at `start` in `haystack` under the case option.
fn matches_at(haystack: &[char], start: usize, needle: &[char], case_sensitive: bool) -> bool {
    haystack.len() >= start + needle.len()
        && haystack[start..]
            .iter()
            .zip(needle)
            .all(|(found, wanted)| found == wanted || (!case_sensitive && found.to_lowercase().eq(wanted.to_lowercase())))
}

/// Whether the characters around a match are letters, digits or underscores.
fn touches_word_char(text: &[char], start: usize, length: usize) -> bool {
    let before = if start == 0 { None } else { Some(text[start - 1]) };
    let after = text.get(start + length).copied();
    is_word_char(before) || is_word_char(after)
}

fn is_word_char(value: Option<char>) -> bool {
    value.is_some_and(|character| character.is_alphanumeric() || character == '_')
}

/// Run-level styling applied by `element.setTextStyle`.
///
/// Every set field is written onto every run of the element; omitted or `null`
/// fields keep the run's current value.
struct TextStyle {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    size_pt: Option<f64>,
    color: Option<String>,
    font_family: Option<String>,
    align: Option<TextAlign>,
}

impl TextStyle {
    fn parse(operation: &Value, operation_name: &str) -> Result<Self, DeckError> {
        let style = Self {
            bold: optional_bool(operation, "bold")?,
            italic: optional_bool(operation, "italic")?,
            underline: optional_bool(operation, "underline")?,
            strike: optional_bool(operation, "strike")?,
            size_pt: optional_font_size(operation)?,
            color: optional_hex_color(operation)?,
            font_family: optional_string(operation, "fontFamily")?.filter(|family| !family.trim().is_empty()),
            align: optional_text_align(operation)?,
        };
        if style.bold.is_none()
            && style.italic.is_none()
            && style.underline.is_none()
            && style.strike.is_none()
            && style.size_pt.is_none()
            && style.color.is_none()
            && style.font_family.is_none()
            && style.align.is_none()
        {
            return Err(invalid(format!("`{operation_name}` needs at least one style property")));
        }
        Ok(style)
    }
}

/// Clones the element's rich text and stamps the requested style onto every run.
fn styled_content(draft: &DeckEdit, slide: &SlideId, element: &ElementId, style: &TextStyle) -> Result<TextContent, DeckError> {
    let model = draft.document().slide(slide).ok_or_else(|| missing_slide(slide))?;
    let target = model.element(element).ok_or_else(|| {
        DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{}` does not exist on this slide", element.as_str()))
            .with_slide(slide.clone())
            .with_element(element.clone())
    })?;
    let ElementContent::Text(text) = &target.content
    else {
        return Err(invalid("`element.setTextStyle` needs a text element"));
    };
    let mut text = text.clone();
    style.apply(&mut text);
    Ok(text)
}

fn hyperlink_content(draft: &DeckEdit, slide: &SlideId, element: &ElementId, link: Option<String>) -> Result<TextContent, DeckError> {
    let model = draft.document().slide(slide).ok_or_else(|| missing_slide(slide))?;
    let target = model.element(element).ok_or_else(|| {
        DeckError::new(codes::OBJECT_NOT_FOUND, format!("element `{}` does not exist on this slide", element.as_str()))
            .with_slide(slide.clone())
            .with_element(element.clone())
    })?;
    let ElementContent::Text(text) = &target.content
    else {
        return Err(invalid("`element.setHyperlink` needs a text element"));
    };
    let mut text = text.clone();
    for paragraph in &mut text.paragraphs {
        for run in &mut paragraph.runs {
            run.hyperlink = link.clone();
        }
    }
    Ok(text)
}

impl TextStyle {
    /// Stamps every set property onto every run of `text`.
    fn apply(&self, text: &mut TextContent) {
        for paragraph in &mut text.paragraphs {
            if let Some(value) = self.align {
                paragraph.align = Some(value);
            }
            for run in &mut paragraph.runs {
                if let Some(value) = self.bold {
                    run.bold = Some(value);
                }
                if let Some(value) = self.italic {
                    run.italic = Some(value);
                }
                if let Some(value) = self.underline {
                    run.underline = Some(value);
                }
                if let Some(value) = self.strike {
                    run.strike = Some(value);
                }
                if let Some(value) = self.size_pt {
                    run.size_pt = Some(value);
                }
                if let Some(value) = &self.color {
                    run.color = Some(value.clone());
                }
                if let Some(value) = &self.font_family {
                    run.font_family = Some(value.clone());
                }
            }
        }
    }
}

fn optional_bool(operation: &Value, key: &str) -> Result<Option<bool>, DeckError> {
    match operation.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(invalid(format!("`{key}` must be a boolean or null"))),
    }
}

/// Reads `sizePt` as a point size the format can store.
fn optional_font_size(operation: &Value) -> Result<Option<f64>, DeckError> {
    match operation.get("sizePt") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|size| size.is_finite() && (1.0..=4000.0).contains(size))
            .map(Some)
            .ok_or_else(|| invalid("`sizePt` must be between 1 and 4000 points")),
    }
}

/// Reads `color` as `#RGB` or `#RRGGBB` and normalizes it to `#RRGGBB`.
fn optional_hex_color(operation: &Value) -> Result<Option<String>, DeckError> {
    let Some(raw) = optional_string(operation, "color")?
    else {
        return Ok(None);
    };
    let trimmed = raw.trim().trim_start_matches('#');
    let expanded = match trimmed.len() {
        3 if trimmed.chars().all(|character| character.is_ascii_hexdigit()) => {
            trimmed.chars().flat_map(|character| [character, character]).collect::<String>()
        }
        6 if trimmed.chars().all(|character| character.is_ascii_hexdigit()) => trimmed.to_string(),
        _ => return Err(invalid("`color` must be a 3- or 6-digit hex colour")),
    };
    Ok(Some(format!("#{}", expanded.to_ascii_uppercase())))
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

/// One alignment edge; only the axis that belongs to it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlignEdge {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

fn parse_align_edge(value: &str) -> Result<AlignEdge, DeckError> {
    Ok(match value {
        "left" => AlignEdge::Left,
        "center" => AlignEdge::Center,
        "right" => AlignEdge::Right,
        "top" => AlignEdge::Top,
        "middle" => AlignEdge::Middle,
        "bottom" => AlignEdge::Bottom,
        other => {
            return Err(invalid(format!(
                "`edge` must be `left`, `center`, `right`, `top`, `middle` or `bottom`, not `{other}`"
            )));
        }
    })
}

/// Reference frame of an alignment run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelativeTo {
    Slide,
    Selection,
}

fn parse_relative_to(operation: &Value) -> Result<RelativeTo, DeckError> {
    match operation.get("relativeTo") {
        None | Some(Value::Null) => Ok(RelativeTo::Slide),
        Some(Value::String(value)) if value == "slide" => Ok(RelativeTo::Slide),
        Some(Value::String(value)) if value == "selection" => Ok(RelativeTo::Selection),
        Some(_) => Err(invalid("`relativeTo` must be `slide` or `selection`")),
    }
}

/// Axis along which elements are distributed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DistributeAxis {
    Horizontal,
    Vertical,
}

fn parse_distribute_axis(value: &str) -> Result<DistributeAxis, DeckError> {
    match value {
        "horizontal" => Ok(DistributeAxis::Horizontal),
        "vertical" => Ok(DistributeAxis::Vertical),
        other => Err(invalid(format!("`axis` must be `horizontal` or `vertical`, not `{other}`"))),
    }
}

impl DistributeAxis {
    fn start(self, transform: &Transform) -> i64 {
        match self {
            Self::Horizontal => transform.x_emu,
            Self::Vertical => transform.y_emu,
        }
    }

    fn set_start(self, transform: &mut Transform, value: i64) {
        match self {
            Self::Horizontal => transform.x_emu = value,
            Self::Vertical => transform.y_emu = value,
        }
    }

    fn size(self, transform: &Transform) -> i64 {
        match self {
            Self::Horizontal => transform.width_emu,
            Self::Vertical => transform.height_emu,
        }
    }
}

/// Frame positions are computed from one snapshot, then written in one pass.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

fn missing_slide(slide: &SlideId) -> DeckError {
    DeckError::new(codes::OBJECT_NOT_FOUND, format!("slide `{slide}` does not exist")).with_slide(slide.clone())
}

fn unknown_element(slide: &SlideId, element: &ElementId) -> DeckError {
    invalid(format!("element `{element}` does not exist on this slide")).with_slide(slide.clone()).with_element(element.clone())
}

fn unsupported_element(slide: &SlideId, element: &ElementId, reason: &str) -> DeckError {
    DeckError::new(codes::OBJECT_UNSUPPORTED, format!("element `{element}` cannot be duplicated: {reason}"))
        .with_slide(slide.clone())
        .with_element(element.clone())
}

fn element_frames(draft: &DeckEdit, slide: &SlideId, elements: &[ElementId]) -> Result<Vec<Transform>, DeckError> {
    let model = draft.document().slide(slide).ok_or_else(|| missing_slide(slide))?;
    elements
        .iter()
        .map(|id| model.element(id).map(|element| element.transform).ok_or_else(|| unknown_element(slide, id)))
        .collect()
}

fn selection_bounds(frames: &[Transform]) -> Bounds {
    let mut x = i64::MAX;
    let mut y = i64::MAX;
    let mut right = i64::MIN;
    let mut bottom = i64::MIN;
    for transform in frames {
        x = x.min(transform.x_emu);
        y = y.min(transform.y_emu);
        right = right.max(transform.x_emu.saturating_add(transform.width_emu));
        bottom = bottom.max(transform.y_emu.saturating_add(transform.height_emu));
    }
    Bounds { x, y, width: right.saturating_sub(x), height: bottom.saturating_sub(y) }
}

/// Aligns the selected boxes to the slide or to their joint bounding box.
fn align_elements(
    draft: &mut DeckEdit,
    slide: &SlideId,
    elements: &[ElementId],
    edge: AlignEdge,
    relative_to: RelativeTo,
) -> Result<(), DeckError> {
    let frames = element_frames(draft, slide, elements)?;
    let bounds = match relative_to {
        RelativeTo::Slide => {
            let size = draft.document().size;
            Bounds { x: 0, y: 0, width: size.width_emu, height: size.height_emu }
        }
        RelativeTo::Selection => selection_bounds(&frames),
    };
    let updates = elements
        .iter()
        .zip(frames)
        .map(|(id, transform)| {
            let mut next = transform;
            match edge {
                AlignEdge::Left => next.x_emu = bounds.x,
                AlignEdge::Center => next.x_emu = bounds.x + (bounds.width - next.width_emu) / 2,
                AlignEdge::Right => next.x_emu = bounds.x + bounds.width - next.width_emu,
                AlignEdge::Top => next.y_emu = bounds.y,
                AlignEdge::Middle => next.y_emu = bounds.y + (bounds.height - next.height_emu) / 2,
                AlignEdge::Bottom => next.y_emu = bounds.y + bounds.height - next.height_emu,
            }
            (id.clone(), next)
        })
        .collect::<Vec<_>>();
    for (id, transform) in updates {
        draft.update_transform(slide, &id, transform)?;
    }
    Ok(())
}

/// Spaces the middle boxes evenly between the fixed first and last boxes.
fn distribute_elements(
    draft: &mut DeckEdit,
    slide: &SlideId,
    elements: &[ElementId],
    axis: DistributeAxis,
) -> Result<(), DeckError> {
    let frames = element_frames(draft, slide, elements)?;
    let mut ordered = elements
        .iter()
        .cloned()
        .zip(frames)
        .enumerate()
        .map(|(index, (id, transform))| (axis.start(&transform), index, id, transform))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(start, index, _, _)| (*start, *index));

    let start = ordered.first().map_or(0, |(start, _, _, _)| *start);
    let end = ordered.last().map_or(start, |(_, _, _, transform)| axis.start(transform).saturating_add(axis.size(transform)));
    let total = ordered.iter().map(|(_, _, _, transform)| axis.size(transform)).sum::<i64>();
    let gaps = (ordered.len() - 1) as i64;
    let spacing = end.saturating_sub(start).saturating_sub(total) / gaps;

    let last = ordered.len() - 1;
    let mut cursor = start;
    let mut updates = Vec::with_capacity(last.saturating_sub(1));
    for (position, (_, _, id, transform)) in ordered.iter().enumerate() {
        if position != 0 && position != last {
            let mut next = *transform;
            axis.set_start(&mut next, cursor);
            updates.push((id.clone(), next));
        }
        cursor = cursor.saturating_add(axis.size(transform)).saturating_add(spacing);
    }
    for (id, transform) in updates {
        draft.update_transform(slide, &id, transform)?;
    }
    Ok(())
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

/// Rows already queued for one table, so a later cell edit starts from them.
fn queued_rows(deferred: &[Deferred], slide: &SlideId, element: &ElementId) -> Option<Vec<TableRow>> {
    deferred.iter().find_map(|entry| match entry {
        Deferred::TableRows { slide: queued_slide, element: queued_element, rows }
            if queued_slide == slide && queued_element == element =>
        {
            Some(rows.clone())
        }
        _ => None,
    })
}

/// Parses the `line` argument of `element.setLine`.
///
/// A null clears the stroke; otherwise at least one of `color` and `widthPt`
/// must be present. The width is a positive point value, stored as EMU.
fn required_line(operation: &Value) -> Result<Option<Line>, DeckError> {
    match operation.get("line") {
        None => Err(invalid("`element.setLine` needs a `line` object or null")),
        Some(Value::Null) => Ok(None),
        Some(value) => {
            let color = match value.get("color") {
                None | Some(Value::Null) => None,
                Some(Value::String(color)) => Some(color.clone()),
                Some(_) => return Err(invalid("`line.color` must be a CSS colour string")),
            };
            let width = match value.get("widthPt") {
                None | Some(Value::Null) => None,
                Some(raw) => {
                    let points = raw
                        .as_f64()
                        .filter(|points| points.is_finite() && *points > 0.0 && *points <= 1584.0)
                        .ok_or_else(|| invalid("`line.widthPt` must be a number between 0 and 1584 points"))?;
                    Some(
                        document_presentation::authored::points_to_emu(points)
                            .ok_or_else(|| invalid("`line.widthPt` is outside the drawable range"))?,
                    )
                }
            };
            if color.is_none() && width.is_none() {
                return Err(invalid("`line` needs a `color` and/or a `widthPt`"));
            }
            Ok(Some(Line { color, width_emu: width }))
        }
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
    Base64::standard()
        .decode(raw.as_bytes())
        .map(|facts| facts.value)
        .map_err(|error| invalid(format!("asset bytes are not valid base64: {error}")))
}

fn element_text(element: &document_presentation::authored::Element) -> Option<String> {
    match &element.content {
        document_presentation::authored::ElementContent::Text(text) => Some(text.text()),
        _ => None,
    }
}

