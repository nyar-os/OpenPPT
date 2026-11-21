//! Static SVG pages from the authored deck model.
//!
//! One page is one standalone SVG document sized at 96 dpi. Text, preset
//! shapes, pictures and tables are painted from the committed revision; charts,
//! formulas, media and groups are skipped and reported as loss codes rather
//! than drawn as placeholders.

use crate::base64_text;
use document_presentation::authored::{
    Deck, Element, ElementContent, Fill, Line, TableContent, TextAlign, TextParagraph, TextRun,
};
use serde_json::{Value, json};

use crate::ServiceError;

/// Loss reported when a shape carries a chart.
pub const LOSS_CHART: &str = "chart";
/// Loss reported when a shape carries a formula.
pub const LOSS_FORMULA: &str = "formula";
/// Loss reported when a shape carries audio or video.
pub const LOSS_MEDIA: &str = "media";
/// Loss reported when a shape is a group.
pub const LOSS_GROUP: &str = "group";
/// Loss reported when a picture references no readable embedded asset.
pub const LOSS_IMAGE: &str = "image";

/// English Metric Units per CSS pixel at 96 dpi.
const EMU_PER_PX: f64 = 9525.0;
/// CSS pixels per typographic point at 96 dpi.
const PX_PER_PT: f64 = 96.0 / 72.0;
/// Inner padding of a text box, in pixels.
const PAD: f64 = 4.0;

/// Renders every slide of `deck` to one standalone SVG document.
pub fn render(deck: &Deck) -> Result<Value, ServiceError> {
    let size = deck.size();
    let width_px = emu_to_px(size.width_emu);
    let height_px = emu_to_px(size.height_emu);
    let mut losses = Vec::new();

    let pages = deck
        .slides()
        .iter()
        .enumerate()
        .map(|(index, slide)| {
            let svg = slide_svg(deck, slide, width_px, height_px, &mut losses);
            json!({
                "slideId": slide.id.as_str(),
                "index": index,
                "widthPx": round2(width_px),
                "heightPx": round2(height_px),
                "svg": svg,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "revision": deck.revision(),
        "format": "svg",
        "pages": pages,
        "losses": losses,
        "diagnostics": [],
    }))
}

fn slide_svg(
    deck: &Deck,
    slide: &document_presentation::authored::DeckSlide,
    width_px: f64,
    height_px: f64,
    losses: &mut Vec<Value>,
) -> String {
    let mut svg = String::with_capacity(1024);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        num(width_px),
        num(height_px),
        num(width_px),
        num(height_px)
    ));
    let background = background_css(slide.background.as_ref());
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        num(width_px),
        num(height_px),
        escape_attr(&background)
    ));

    for element in &slide.elements {
        if let Some(painted) = element_svg(deck, slide, element, losses) {
            svg.push_str(&painted);
        }
    }

    svg.push_str("</svg>");
    svg
}

fn element_svg(
    deck: &Deck,
    slide: &document_presentation::authored::DeckSlide,
    element: &Element,
    losses: &mut Vec<Value>,
) -> Option<String> {
    let frame = Frame::from_element(element);
    let inner = match &element.content {
        ElementContent::Text(text) => text_svg(element, text, frame),
        ElementContent::Shape(shape) => shape_svg(shape.geometry.as_str(), shape.fill.as_ref(), shape.line.as_ref(), frame),
        ElementContent::Image(image) => match image_svg(deck, &image.asset, frame) {
            Some(svg) => svg,
            None => {
                losses.push(loss(slide, element, LOSS_IMAGE, "the referenced image asset has no readable bytes"));
                return None;
            }
        },
        ElementContent::Table(table) => table_svg(table, frame),
        ElementContent::Chart(_) => {
            losses.push(loss(slide, element, LOSS_CHART, "charts are kept as editable data and are not painted as SVG"));
            return None;
        }
        ElementContent::Formula(_) => {
            losses.push(loss(slide, element, LOSS_FORMULA, "formulas keep their source expression and are not painted as SVG"));
            return None;
        }
        ElementContent::Media(_) => {
            losses.push(loss(slide, element, LOSS_MEDIA, "audio and video have no static SVG representation"));
            return None;
        }
    };

    Some(match element.transform.rotation_deg {
        Some(rotation) if rotation.abs() > f64::EPSILON => {
            let cx = frame.x + frame.w / 2.0;
            let cy = frame.y + frame.h / 2.0;
            format!("<g transform=\"rotate({} {} {})\">{inner}</g>", num(rotation), num(cx), num(cy))
        }
        _ => inner,
    })
}

fn text_svg(element: &Element, text: &document_presentation::authored::TextContent, frame: Frame) -> String {
    let mut svg = String::with_capacity(512);
    if let Some(fill) = &element.style.fill
        && let Some(color) = fill_solid(fill)
    {
        svg.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            num(frame.x),
            num(frame.y),
            num(frame.w),
            num(frame.h),
            escape_attr(&color)
        ));
    }
    if let Some(line) = &element.style.line
        && let Some(stroke) = stroke_svg(line)
    {
        svg.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" {stroke}/>",
            num(frame.x),
            num(frame.y),
            num(frame.w),
            num(frame.h)
        ));
    }
    text_body(&mut svg, &text.paragraphs, frame, 18.0, element.style.font_family.as_deref());
    svg
}

fn shape_svg(geometry: &str, fill: Option<&Fill>, line: Option<&Line>, frame: Frame) -> String {
    let fill = fill.and_then(fill_solid).unwrap_or_else(|| "none".to_string());
    let stroke = line.and_then(stroke_svg).map(|value| format!(" {value}")).unwrap_or_default();
    let paint = format!("fill=\"{}\"{stroke}", escape_attr(&fill));
    match geometry {
        "ellipse" => format!(
            "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" {paint}/>",
            num(frame.x + frame.w / 2.0),
            num(frame.y + frame.h / 2.0),
            num(frame.w / 2.0),
            num(frame.h / 2.0)
        ),
        "roundRect" => {
            let radius = frame.w.min(frame.h) * 0.1;
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" ry=\"{}\" {paint}/>",
                num(frame.x),
                num(frame.y),
                num(frame.w),
                num(frame.h),
                num(radius),
                num(radius)
            )
        }
        _ => format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {paint}/>",
            num(frame.x),
            num(frame.y),
            num(frame.w),
            num(frame.h)
        ),
    }
}

fn image_svg(deck: &Deck, asset: &document_presentation::authored::AssetId, frame: Frame) -> Option<String> {
    let asset = deck.asset(asset)?;
    if asset.bytes.is_empty() {
        return None;
    }
    let href = format!("data:{};base64,{}", asset.media_type, base64_text(&asset.bytes));
    Some(format!(
        "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" xlink:href=\"{}\" href=\"{}\"/>",
        num(frame.x),
        num(frame.y),
        num(frame.w),
        num(frame.h),
        escape_attr(&href),
        escape_attr(&href)
    ))
}

fn table_svg(table: &TableContent, frame: Frame) -> String {
    let mut svg = String::with_capacity(512);
    let columns = table.column_widths_emu.len().max(1);
    let default_width = frame.w / columns as f64;
    let mut top = frame.y;
    for row in &table.rows {
        let height = emu_to_px(row.height_emu);
        let mut left = frame.x;
        for (index, cell) in row.cells.iter().enumerate() {
            let width = table.column_widths_emu.get(index).map(|emu| emu_to_px(*emu)).unwrap_or(default_width);
            let fill = cell.fill.as_ref().and_then(fill_solid).unwrap_or_else(|| "none".to_string());
            svg.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"#9CA3AF\" stroke-width=\"0.75\"/>",
                num(left),
                num(top),
                num(width),
                num(height),
                escape_attr(&fill)
            ));
            let cell_frame = Frame { x: left, y: top, w: width, h: height };
            text_body(&mut svg, &cell.text.paragraphs, cell_frame, 10.0, None);
            left += width;
        }
        top += height;
    }
    svg
}

fn text_body(out: &mut String, paragraphs: &[TextParagraph], frame: Frame, default_size_pt: f64, family: Option<&str>) {
    let mut top = frame.y + PAD;
    for paragraph in paragraphs {
        let size = paragraph.runs.iter().filter_map(|run| run.size_pt).fold(default_size_pt, f64::max);
        let line_height = size * PX_PER_PT * 1.25;
        let (anchor, x) = match paragraph.align.unwrap_or(TextAlign::Left) {
            TextAlign::Center => ("middle", frame.x + frame.w / 2.0),
            TextAlign::Right => ("end", frame.x + frame.w - PAD),
            TextAlign::Left | TextAlign::Justify => ("start", frame.x + PAD),
        };
        let mut lines: Vec<Vec<(&TextRun, &str)>> = vec![Vec::new()];
        for run in &paragraph.runs {
            for (index, segment) in run.text.split('\n').enumerate() {
                if index > 0 {
                    lines.push(Vec::new());
                }
                if !segment.is_empty()
                    && let Some(line) = lines.last_mut()
                {
                    line.push((run, segment));
                }
            }
        }
        for line in lines {
            if line.is_empty() {
                top += line_height;
                continue;
            }
            let baseline = top + line_height - size * PX_PER_PT * 0.25;
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" font-family=\"{}\" font-size=\"{}\" fill=\"#000000\" xml:space=\"preserve\">",
                num(x),
                num(baseline),
                escape_attr(family.unwrap_or("Segoe UI, system-ui, sans-serif")),
                num(size * PX_PER_PT)
            ));
            for (run, segment) in line {
                out.push_str("<tspan");
                if let Some(run_size) = run.size_pt {
                    out.push_str(&format!(" font-size=\"{}\"", num(run_size * PX_PER_PT)));
                }
                if run.bold.unwrap_or(false) {
                    out.push_str(" font-weight=\"bold\"");
                }
                if run.italic.unwrap_or(false) {
                    out.push_str(" font-style=\"italic\"");
                }
                if let Some(color) = &run.color {
                    out.push_str(&format!(" fill=\"{}\"", escape_attr(color)));
                }
                if let Some(run_family) = &run.font_family {
                    out.push_str(&format!(" font-family=\"{}\"", escape_attr(run_family)));
                }
                if run.underline.unwrap_or(false) {
                    out.push_str(" text-decoration=\"underline\"");
                }
                out.push_str(&format!(">{}</tspan>", escape_text(segment)));
            }
            out.push_str("</text>");
            top += line_height;
        }
    }
}

fn loss(slide: &document_presentation::authored::DeckSlide, element: &Element, code: &str, message: &str) -> Value {
    json!({
        "code": code,
        "slideId": slide.id.as_str(),
        "elementId": element.id.as_str(),
        "message": message,
    })
}

fn background_css(fill: Option<&Fill>) -> String {
    match fill {
        Some(Fill::Solid { color }) => color.clone(),
        Some(Fill::None) => "none".to_string(),
        Some(Fill::Inherit) | None => "#FFFFFF".to_string(),
    }
}

fn fill_solid(fill: &Fill) -> Option<String> {
    match fill {
        Fill::Solid { color } => Some(color.clone()),
        Fill::None | Fill::Inherit => None,
    }
}

fn stroke_svg(line: &Line) -> Option<String> {
    let width = line.width_emu.map(emu_to_px).unwrap_or(1.0).max(0.0);
    let color = line.color.as_deref().unwrap_or("#000000");
    Some(format!("stroke=\"{}\" stroke-width=\"{}\"", escape_attr(color), num(width)))
}

fn emu_to_px(emu: i64) -> f64 {
    emu as f64 / EMU_PER_PX
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let text = format!("{}", round2(value));
    if text == "-0" { "0".to_string() } else { text }
}

fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(character),
        }
    }
    out
}

fn escape_attr(value: &str) -> String {
    escape_text(value)
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Frame {
    fn from_element(element: &Element) -> Self {
        Self {
            x: emu_to_px(element.transform.x_emu),
            y: emu_to_px(element.transform.y_emu),
            w: emu_to_px(element.transform.width_emu),
            h: emu_to_px(element.transform.height_emu),
        }
    }
}

#[cfg(test)]
mod tests {
    use document_presentation::authored::{
        ChartData, ChartElementInput, ChartSeries, ChartType, Deck, DeckCreateOptions, ShapeElementInput, SlideAddOptions,
        TextContent, TextElementInput, TextParagraph, TextRun, Transform,
    };

    use super::*;

    #[test]
    fn svg_pages_paint_text_shapes_and_report_losses() {
        let mut deck = Deck::create(DeckCreateOptions { title: Some("Render".to_string()), ..DeckCreateOptions::default() });
        deck.transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent {
                        paragraphs: vec![TextParagraph {
                            runs: vec![TextRun {
                                text: "A & B < C\nsecond line".to_string(),
                                bold: Some(true),
                                italic: Some(true),
                                size_pt: Some(24.0),
                                color: Some("#FF0000".to_string()),
                                ..TextRun::plain("")
                            }],
                            level: 0,
                            align: Some(TextAlign::Center),
                        }],
                    },
                    transform: Some(Transform::from_points(60.0, 60.0, 400.0, 120.0).unwrap()),
                    alt: Some("Greeting".to_string()),
                    placeholder: None,
                },
            )?;
            edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "ellipse".to_string(),
                    transform: Some(Transform::from_points(60.0, 200.0, 200.0, 120.0).unwrap()),
                    fill: Some(Fill::Solid { color: "#00FF00".to_string() }),
                    line: None,
                    alt: None,
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
                    transform: Some(Transform::from_points(300.0, 200.0, 200.0, 150.0).unwrap()),
                    alt: "Sales".to_string(),
                },
            )?;
            Ok(())
        })
        .expect("render deck builds");

        let report = render(&deck).expect("render succeeds");
        assert_eq!(report["format"], "svg");
        let pages = report["pages"].as_array().expect("pages");
        assert_eq!(pages.len(), 1);
        assert!((pages[0]["widthPx"].as_f64().unwrap() - 1280.0).abs() < 0.05);
        assert!((pages[0]["heightPx"].as_f64().unwrap() - 720.0).abs() < 0.05);
        let svg = pages[0]["svg"].as_str().expect("svg");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("viewBox=\"0 0 1280 720\""));
        assert!(svg.contains("A &amp; B &lt; C"), "text must be XML escaped");
        assert!(svg.contains("second line"));
        assert!(svg.contains("font-weight=\"bold\""));
        assert!(svg.contains("font-style=\"italic\""));
        assert!(svg.contains("text-anchor=\"middle\""));
        assert!(svg.contains("<ellipse"));
        assert!(svg.contains("fill=\"#00FF00\""));

        let losses = report["losses"].as_array().expect("losses");
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0]["code"], LOSS_CHART);
    }
}
