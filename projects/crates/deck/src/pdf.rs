//! Minimal PDF 1.7 writer for a static slide projection.
//!
//! One slide becomes one page sized from the deck in points. Backgrounds,
//! preset shapes (rectangles and ellipses), text, tables, JPEG pictures and
//! PNG pictures are painted. Charts, formulas, media and groups are skipped and
//! reported; PNG pictures are decoded into an image XObject, with an 8-bit
//! `/SMask` when the source carries transparency. Other picture encodings are
//! reported instead of being decoded into a fabricated bitmap. Text is written
//! with the three built-in Helvetica faces in WinAnsi, so characters outside
//! Latin-1 become `?` and are reported as `pdf.glyph`.

use std::{collections::HashMap, path::Path};

use data_compression::{CodecLimits, DeflateLevel, Zlib};
use document_presentation::authored::{
    Deck, DeckSlide, Element, ElementContent, Fill, Line, TableContent, TextAlign, TextParagraph, Transform,
};
use serde_json::{Value, json};

use crate::ServiceError;

/// Loss reported when a picture cannot be embedded as a JPEG or PNG stream.
pub(crate) const LOSS_IMAGE: &str = "image";
/// Loss reported when text contains a character outside WinAnsi/Latin-1.
pub(crate) const LOSS_GLYPH: &str = "glyph";
/// Loss reported when a chart is skipped.
pub(crate) const LOSS_CHART: &str = "chart";
/// Loss reported when a formula is skipped.
pub(crate) const LOSS_FORMULA: &str = "formula";
/// Loss reported when audio or video is skipped.
pub(crate) const LOSS_MEDIA: &str = "media";
/// Loss reported when a group shape is skipped.
pub(crate) const LOSS_GROUP: &str = "group";

/// English Metric Units per typographic point.
const EMU_PER_PT: f64 = 12_700.0;
/// Inner padding of a text box, in points.
const PAD: f64 = 4.0;
/// Default text size when no run declares one, in points.
const DEFAULT_TEXT_PT: f64 = 18.0;
/// Default table cell text size, in points.
const CELL_TEXT_PT: f64 = 10.0;

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

    let (bytes, losses) = build(deck)?;
    if loss_policy == "error" && !losses.is_empty() {
        return Err(ServiceError::new(
            "export/loss-exceeded",
            format!("the pdf projection would drop {} construct(s)", losses.len()),
        ));
    }
    crate::write_atomically(&bytes, Path::new(target), overwrite)?;

    Ok(json!({
        "revision": deck.revision(),
        "format": "pdf",
        "target": target,
        "bytes": bytes.len(),
        "editable": false,
        "losses": losses,
        "diagnostics": [],
    }))
}

fn build(deck: &Deck) -> Result<(Vec<u8>, Vec<Value>), ServiceError> {
    let size = deck.size();
    let width = emu_to_pt(size.width_emu);
    let height = emu_to_pt(size.height_emu);
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let pages = pdf.reserve();
    let regular = pdf.reserve();
    let bold = pdf.reserve();
    let oblique = pdf.reserve();
    pdf.put(regular, font_object("Helvetica"));
    pdf.put(bold, font_object("Helvetica-Bold"));
    pdf.put(oblique, font_object("Helvetica-Oblique"));

    let mut losses = Vec::new();
    let mut images: HashMap<String, usize> = HashMap::new();
    let mut page_ids = Vec::new();

    for slide in deck.slides() {
        let mut content = Vec::new();
        paint_background(&mut content, slide, width, height);
        let mut xobjects: Vec<(String, usize)> = Vec::new();

        for element in &slide.elements {
            match &element.content {
                ElementContent::Text(text) => {
                    let frame = Frame::from(element.transform);
                    paint_text(&mut content, &text.paragraphs, frame, DEFAULT_TEXT_PT, height);
                    if paragraph_glyph_loss(&text.paragraphs) {
                        losses.push(loss(slide, element, LOSS_GLYPH, "text outside Latin-1 is written as `?`"));
                    }
                }
                ElementContent::Shape(shape) => {
                    paint_shape(
                        &mut content,
                        &shape.geometry,
                        shape.fill.as_ref(),
                        shape.line.as_ref(),
                        Frame::from(element.transform),
                        height,
                    );
                }
                ElementContent::Image(image) => {
                    let Some(asset) = deck.asset(&image.asset)
                    else {
                        losses.push(loss(slide, element, LOSS_IMAGE, "the referenced image asset is missing"));
                        continue;
                    };
                    let Some((image_width, image_height, components)) = jpeg_info(&asset.bytes)
                    else {
                        let object = match images.get(image.asset.as_str()) {
                            Some(object) => *object,
                            None => match image_object(&mut pdf, &asset.bytes) {
                                Ok(object) => {
                                    images.insert(image.asset.as_str().to_string(), object);
                                    object
                                }
                                Err(message) => {
                                    losses.push(loss(slide, element, LOSS_IMAGE, &message));
                                    continue;
                                }
                            },
                        };
                        let name = format!("Im{}", xobjects.len() + 1);
                        let frame = Frame::from(element.transform);
                        content.extend_from_slice(
                            format!(
                                "q {} 0 0 {} {} {} cm /{} Do Q\n",
                                num(frame.w),
                                num(frame.h),
                                num(frame.x),
                                num(height - frame.y - frame.h),
                                name
                            )
                            .as_bytes(),
                        );
                        xobjects.push((name, object));
                        continue;
                    };
                    let object = match images.get(image.asset.as_str()) {
                        Some(object) => *object,
                        None => {
                            let object = pdf.reserve();
                            pdf.put(object, jpeg_object(&asset.bytes, image_width, image_height, components));
                            images.insert(image.asset.as_str().to_string(), object);
                            object
                        }
                    };
                    let name = format!("Im{}", xobjects.len() + 1);
                    let frame = Frame::from(element.transform);
                    content.extend_from_slice(
                        format!(
                            "q {} 0 0 {} {} {} cm /{} Do Q\n",
                            num(frame.w),
                            num(frame.h),
                            num(frame.x),
                            num(height - frame.y - frame.h),
                            name
                        )
                        .as_bytes(),
                    );
                    xobjects.push((name, object));
                }
                ElementContent::Table(table) => {
                    paint_table(&mut content, table, Frame::from(element.transform), height, &mut losses, slide, element);
                }
                ElementContent::Chart(_) => {
                    losses.push(loss(
                        slide,
                        element,
                        LOSS_CHART,
                        "charts are kept as editable data and are not painted in PDF",
                    ));
                }
                ElementContent::Formula(_) => {
                    losses.push(loss(
                        slide,
                        element,
                        LOSS_FORMULA,
                        "formulas keep their source expression and are not painted in PDF",
                    ));
                }
                ElementContent::Media(_) => {
                    losses.push(loss(slide, element, LOSS_MEDIA, "audio and video have no static PDF representation"));
                }
            }
        }

        let content_id = pdf.reserve();
        pdf.put(content_id, stream_object("", &content));
        let page_id = pdf.reserve();
        let mut resources = format!("<< /Font << /F1 {regular} 0 R /F2 {bold} 0 R /F3 {oblique} 0 R >>");
        if !xobjects.is_empty() {
            resources.push_str(" /XObject <<");
            for (name, object) in &xobjects {
                resources.push_str(&format!(" /{name} {object} 0 R"));
            }
            resources.push_str(" >>");
        }
        resources.push_str(" >>");
        pdf.put(
            page_id,
            format!(
                "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {} {}] /Resources {resources} /Contents {content_id} 0 R >>",
                num(width),
                num(height)
            )
            .into_bytes(),
        );
        page_ids.push(page_id);
    }

    let kids = page_ids.iter().map(|id| format!("{id} 0 R")).collect::<Vec<_>>().join(" ");
    pdf.put(pages, format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", page_ids.len()).into_bytes());
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>").into_bytes());

    Ok((pdf.finish(catalog), losses))
}

fn paint_background(out: &mut Vec<u8>, slide: &DeckSlide, width: f64, height: f64) {
    let Some(Fill::Solid { color }) = &slide.background
    else {
        return;
    };
    let Some((red, green, blue)) = color_rgb(color)
    else {
        return;
    };
    out.extend_from_slice(
        format!("{} {} {} rg 0 0 {} {} re f\n", num(red), num(green), num(blue), num(width), num(height)).as_bytes(),
    );
}

fn paint_shape(out: &mut Vec<u8>, geometry: &str, fill: Option<&Fill>, line: Option<&Line>, frame: Frame, page_height: f64) {
    let fill_color = fill.and_then(solid_rgb);
    let stroke = line.and_then(|line| {
        let color = line.color.as_deref().and_then(color_rgb).unwrap_or((0.0, 0.0, 0.0));
        let width = line.width_emu.map(emu_to_pt).unwrap_or(1.0).max(0.0);
        (width > 0.0).then_some((color, width))
    });
    if fill_color.is_none() && stroke.is_none() {
        return;
    }

    if geometry == "ellipse" {
        let (cx, cy, rx, ry) = (frame.x + frame.w / 2.0, page_height - frame.y - frame.h / 2.0, frame.w / 2.0, frame.h / 2.0);
        let k = 0.552_284_75;
        out.extend_from_slice(
            format!(
                "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c\n",
                num(cx),
                num(cy + ry),
                num(cx + rx * k),
                num(cy + ry),
                num(cx + rx),
                num(cy + ry * k),
                num(cx + rx),
                num(cy),
                num(cx + rx),
                num(cy - ry * k),
                num(cx + rx * k),
                num(cy - ry),
                num(cx),
                num(cy - ry),
                num(cx - rx * k),
                num(cy - ry),
                num(cx - rx),
                num(cy - ry * k),
                num(cx - rx),
                num(cy),
                num(cx - rx),
                num(cy + ry * k),
                num(cx - rx * k),
                num(cy + ry),
                num(cx),
                num(cy + ry)
            )
            .as_bytes(),
        );
    }
    else {
        out.extend_from_slice(
            format!("{} {} {} {} re\n", num(frame.x), num(page_height - frame.y - frame.h), num(frame.w), num(frame.h))
                .as_bytes(),
        );
    }

    apply_paint(out, fill_color, stroke);
}

fn apply_paint(out: &mut Vec<u8>, fill: Option<(f64, f64, f64)>, stroke: Option<((f64, f64, f64), f64)>) {
    if let Some((red, green, blue)) = fill {
        out.extend_from_slice(format!("{} {} {} rg\n", num(red), num(green), num(blue)).as_bytes());
    }
    if let Some((color, width)) = stroke {
        out.extend_from_slice(format!("{} {} {} RG {} w\n", num(color.0), num(color.1), num(color.2), num(width)).as_bytes());
    }
    let operator = match (fill, stroke) {
        (Some(_), Some(_)) => "B",
        (Some(_), None) => "f",
        (None, Some(_)) => "S",
        (None, None) => return,
    };
    out.extend_from_slice(format!("{operator}\n").as_bytes());
}

fn paint_text(out: &mut Vec<u8>, paragraphs: &[TextParagraph], frame: Frame, default_size: f64, page_height: f64) {
    let mut top = frame.y + PAD;
    for paragraph in paragraphs {
        let size = paragraph.runs.iter().filter_map(|run| run.size_pt).fold(default_size, f64::max);
        let line_height = size * 1.25;
        let bold = paragraph.runs.iter().any(|run| run.bold.unwrap_or(false));
        let italic = paragraph.runs.iter().any(|run| run.italic.unwrap_or(false));
        let font = if bold {
            "F2"
        }
        else if italic {
            "F3"
        }
        else {
            "F1"
        };
        let color = paragraph.runs.iter().find_map(|run| run.color.as_deref()).and_then(color_rgb).unwrap_or((0.0, 0.0, 0.0));
        let text = paragraph.runs.iter().map(|run| run.text.as_str()).collect::<String>();

        for line in text.split('\n') {
            let estimated = estimate_width(line, size);
            let x = match paragraph.align.unwrap_or(TextAlign::Left) {
                TextAlign::Center => frame.x + (frame.w - estimated).max(0.0) / 2.0,
                TextAlign::Right => frame.x + (frame.w - estimated - PAD).max(0.0),
                TextAlign::Left | TextAlign::Justify => frame.x + PAD,
            };
            let baseline = page_height - (top + line_height - size * 0.25);
            out.extend_from_slice(
                format!(
                    "BT /{font} {} Tf {} {} {} rg {} {} Td ",
                    num(size),
                    num(color.0),
                    num(color.1),
                    num(color.2),
                    num(x),
                    num(baseline)
                )
                .as_bytes(),
            );
            out.push(b'(');
            write_pdf_text(out, line);
            out.extend_from_slice(b") Tj ET\n");
            top += line_height;
        }
    }
}

fn paint_table(
    out: &mut Vec<u8>,
    table: &TableContent,
    frame: Frame,
    page_height: f64,
    losses: &mut Vec<Value>,
    slide: &DeckSlide,
    element: &Element,
) {
    let columns = table.column_widths_emu.len().max(1);
    let default_width = frame.w / columns as f64;
    let mut top = frame.y;
    for row in &table.rows {
        let height = emu_to_pt(row.height_emu);
        let mut left = frame.x;
        for (index, cell) in row.cells.iter().enumerate() {
            let width = table.column_widths_emu.get(index).map(|emu| emu_to_pt(*emu)).unwrap_or(default_width);
            let bottom = page_height - top - height;
            if let Some((red, green, blue)) = cell.fill.as_ref().and_then(solid_rgb) {
                out.extend_from_slice(
                    format!(
                        "{} {} {} rg {} {} {} {} re f\n",
                        num(red),
                        num(green),
                        num(blue),
                        num(left),
                        num(bottom),
                        num(width),
                        num(height)
                    )
                    .as_bytes(),
                );
            }
            out.extend_from_slice(
                format!("0 0 0 RG 0.5 w {} {} {} {} re S\n", num(left), num(bottom), num(width), num(height)).as_bytes(),
            );
            let cell_frame = Frame { x: left, y: top, w: width, h: height };
            paint_text(out, &cell.text.paragraphs, cell_frame, CELL_TEXT_PT, page_height);
            if paragraph_glyph_loss(&cell.text.paragraphs) {
                losses.push(loss(slide, element, LOSS_GLYPH, "table text outside Latin-1 is written as `?`"));
            }
            left += width;
        }
        top += height;
    }
}

/// Writes one paragraph's text as a PDF literal-string body.
fn write_pdf_text(out: &mut Vec<u8>, value: &str) {
    for character in value.chars() {
        match character {
            '\\' => out.extend_from_slice(b"\\\\"),
            '(' => out.extend_from_slice(b"\\("),
            ')' => out.extend_from_slice(b"\\)"),
            '\n' | '\r' | '\t' => out.push(b' '),
            character if (character as u32) < 0x20 => out.push(b' '),
            // Latin-1 is the WinAnsi range this writer can express with a
            // single byte. Anything wider becomes `?` and is reported.
            character if (character as u32) <= 0xFF => out.push(character as u8),
            _ => out.push(b'?'),
        }
    }
}

fn paragraph_glyph_loss(paragraphs: &[TextParagraph]) -> bool {
    paragraphs
        .iter()
        .any(|paragraph| paragraph.runs.iter().any(|run| run.text.chars().any(|character| (character as u32) > 0x7E)))
}

fn estimate_width(text: &str, size: f64) -> f64 {
    text.chars().count() as f64 * size * 0.5
}

fn solid_rgb(fill: &Fill) -> Option<(f64, f64, f64)> {
    match fill {
        Fill::Solid { color } => color_rgb(color),
        Fill::None | Fill::Inherit => None,
    }
}

fn color_rgb(value: &str) -> Option<(f64, f64, f64)> {
    let trimmed = value.trim().trim_start_matches('#');
    let expanded = match trimmed.len() {
        3 if trimmed.chars().all(|character| character.is_ascii_hexdigit()) => {
            trimmed.chars().flat_map(|character| [character, character]).collect::<String>()
        }
        6 | 8 => trimmed.to_string(),
        _ => return None,
    };
    let component =
        |offset: usize| u8::from_str_radix(&expanded[offset..offset + 2], 16).ok().map(|value| value as f64 / 255.0);
    Some((component(0)?, component(2)?, component(4)?))
}

fn jpeg_info(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut index = 2;
    while index + 4 <= bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            index += 2;
            continue;
        }
        if index + 4 > bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        if length < 2 {
            return None;
        }
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            if index + 10 > bytes.len() {
                return None;
            }
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]) as u32;
            let components = bytes[index + 9];
            return (width > 0 && height > 0).then_some((width, height, components));
        }
        if marker == 0xDA {
            return None;
        }
        index += 2 + length;
    }
    None
}

fn jpeg_object(bytes: &[u8], width: u32, height: u32, components: u8) -> Vec<u8> {
    let space = match components {
        1 => "/DeviceGray",
        4 => "/DeviceCMYK",
        _ => "/DeviceRGB",
    };
    stream_object(
        &format!(
            "/Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace {space} /BitsPerComponent 8 /Filter /DCTDecode"
        ),
        bytes,
    )
}

/// PNG file signature; every chunk stream starts with these eight bytes.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// Largest picture dimension this writer is willing to decode.
const PNG_MAX_DIMENSION: u32 = 20_000;
/// Largest decoded raster this writer is willing to hold in memory.
const PNG_MAX_SAMPLES: u64 = 256 * 1024 * 1024;

/// One decoded PNG, already reduced to 8-bit gray or RGB samples plus alpha.
struct PngRaster {
    width: u32,
    height: u32,
    gray: bool,
    samples: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

/// Builds an image XObject for a PNG or JPEG picture, or explains why it cannot.
fn image_object(pdf: &mut Pdf, bytes: &[u8]) -> Result<usize, String> {
    if bytes.starts_with(&PNG_SIGNATURE) {
        let raster = decode_png(bytes)?;
        return Ok(png_object(pdf, &raster));
    }
    Err("only JPEG and PNG pictures are embedded; other encodings are skipped".to_string())
}

/// Writes the image stream (and its 8-bit `/SMask` when transparent) as objects.
fn png_object(pdf: &mut Pdf, raster: &PngRaster) -> usize {
    let mask = raster.alpha.as_ref().map(|alpha| {
        let object = pdf.reserve();
        pdf.put(
            object,
            stream_object(
                &format!(
                    "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode",
                    raster.width, raster.height
                ),
                &Zlib::encode(alpha, DeflateLevel::Default),
            ),
        );
        object
    });

    let space = if raster.gray {
        "/DeviceGray"
    }
    else {
        "/DeviceRGB"
    };
    let mut dictionary = format!(
        "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {space} /BitsPerComponent 8 /Filter /FlateDecode",
        raster.width, raster.height
    );
    if let Some(mask) = mask {
        dictionary.push_str(&format!(" /SMask {mask} 0 R"));
    }

    let object = pdf.reserve();
    pdf.put(object, stream_object(&dictionary, &Zlib::encode(&raster.samples, DeflateLevel::Default)));
    object
}

/// Decodes an 8-bit, non-interlaced PNG into RGB or gray samples.
fn decode_png(bytes: &[u8]) -> Result<PngRaster, String> {
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err("the picture is not a PNG stream".to_string());
    }

    let mut offset = PNG_SIGNATURE.len();
    let mut header: Option<(u32, u32, u8, u8)> = None;
    let mut palette: Option<Vec<u8>> = None;
    let mut transparency: Option<Vec<u8>> = None;
    let mut data = Vec::new();
    let mut saw_end = false;

    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
        let kind = &bytes[offset + 4..offset + 8];
        let start = offset + 8;
        let end = start.checked_add(length).ok_or_else(|| "PNG chunk length overflows".to_string())?;
        let next = end.checked_add(4).ok_or_else(|| "PNG chunk length overflows".to_string())?;
        if next > bytes.len() {
            return Err("PNG chunk data is truncated".to_string());
        }
        let chunk = &bytes[start..end];

        match kind {
            b"IHDR" => {
                if chunk.len() != 13 {
                    return Err("PNG IHDR must be 13 bytes".to_string());
                }
                let width = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let height = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                let bit_depth = chunk[8];
                let color_type = chunk[9];
                let compression = chunk[10];
                let filter = chunk[11];
                let interlace = chunk[12];
                if width == 0 || height == 0 || width > PNG_MAX_DIMENSION || height > PNG_MAX_DIMENSION {
                    return Err("PNG dimensions are outside the supported range".to_string());
                }
                if bit_depth != 8 {
                    return Err(format!("PNG bit depth {bit_depth} is not supported; only 8-bit samples are embedded"));
                }
                if !matches!(color_type, 0 | 2 | 3 | 4 | 6) {
                    return Err(format!("PNG colour type {color_type} is not supported"));
                }
                if compression != 0 || filter != 0 {
                    return Err("PNG compression and filter methods must be standard".to_string());
                }
                if interlace != 0 {
                    return Err("interlaced PNG pictures are not supported".to_string());
                }
                header = Some((width, height, bit_depth, color_type));
            }
            b"PLTE" => palette = Some(chunk.to_vec()),
            b"tRNS" => transparency = Some(chunk.to_vec()),
            b"IDAT" => data.extend_from_slice(chunk),
            b"IEND" => {
                saw_end = true;
                break;
            }
            _ => {}
        }
        offset = next;
    }

    let Some((width, height, _, color_type)) = header
    else {
        return Err("PNG IHDR is missing".to_string());
    };
    if !saw_end {
        return Err("PNG IEND is missing".to_string());
    }
    if data.is_empty() {
        return Err("PNG image data is missing".to_string());
    }

    let channels = match color_type {
        0 => 1usize,
        2 => 3,
        3 => 1,
        4 => 2,
        _ => 4,
    };
    let stride = (width as usize)
        .checked_mul(channels)
        .ok_or_else(|| "PNG raster width overflows".to_string())?;
    let expected = (stride + 1)
        .checked_mul(height as usize)
        .ok_or_else(|| "PNG raster size overflows".to_string())?;
    if expected as u64 > PNG_MAX_SAMPLES {
        return Err("PNG raster is larger than this writer decodes".to_string());
    }

    let raw = Zlib::decode(&data, CodecLimits::new().with_max_output_bytes(expected as u64))
        .map_err(|error| format!("PNG image data could not be inflated: {error}"))?;
    if raw.value.len() != expected {
        return Err("PNG image data has the wrong size for its header".to_string());
    }
    let rows = unfilter(&raw.value, stride, height as usize, channels)
        .map_err(|error| format!("PNG image data could not be reconstructed: {error}"))?;

    let raster = match color_type {
        0 => PngRaster { width, height, gray: true, samples: rows, alpha: None },
        2 => PngRaster { width, height, gray: false, samples: rows, alpha: None },
        3 => {
            let palette = palette.ok_or_else(|| "indexed PNG pictures need a palette".to_string())?;
            let entries = palette.len() / 3;
            if entries == 0 {
                return Err("indexed PNG palette is empty".to_string());
            }
            let transparency = transparency.unwrap_or_default();
            let mut samples = Vec::with_capacity(rows.len() * 3);
            let mut alpha = Vec::with_capacity(rows.len());
            let mut transparent = false;
            for index in rows {
                let index = index as usize;
                if index >= entries {
                    return Err("indexed PNG pixel is outside the palette".to_string());
                }
                samples.extend_from_slice(&palette[index * 3..index * 3 + 3]);
                let value = transparency.get(index).copied().unwrap_or(0xFF);
                transparent |= value != 0xFF;
                alpha.push(value);
            }
            PngRaster {
                width,
                height,
                gray: false,
                samples,
                alpha: transparent.then_some(alpha),
            }
        }
        4 => {
            let mut samples = Vec::with_capacity(rows.len() / 2);
            let mut alpha = Vec::with_capacity(rows.len() / 2);
            let mut transparent = false;
            for pair in rows.chunks_exact(2) {
                samples.push(pair[0]);
                transparent |= pair[1] != 0xFF;
                alpha.push(pair[1]);
            }
            PngRaster { width, height, gray: true, samples, alpha: transparent.then_some(alpha) }
        }
        _ => {
            let mut samples = Vec::with_capacity(rows.len() / 4 * 3);
            let mut alpha = Vec::with_capacity(rows.len() / 4);
            let mut transparent = false;
            for pixel in rows.chunks_exact(4) {
                samples.extend_from_slice(&pixel[..3]);
                transparent |= pixel[3] != 0xFF;
                alpha.push(pixel[3]);
            }
            PngRaster { width, height, gray: false, samples, alpha: transparent.then_some(alpha) }
        }
    };

    Ok(raster)
}

/// Reverses PNG row filters 0-4 and returns the concatenated pixel rows.
fn unfilter(data: &[u8], stride: usize, height: usize, bytes_per_pixel: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(stride * height);
    let mut previous = vec![0u8; stride];
    let mut offset = 0usize;
    for _ in 0..height {
        if offset >= data.len() {
            return Err("row filter byte is missing".to_string());
        }
        let filter = data[offset];
        offset += 1;
        if offset + stride > data.len() {
            return Err("row data is truncated".to_string());
        }
        let mut row = data[offset..offset + stride].to_vec();
        offset += stride;

        for index in 0..stride {
            let left = if index >= bytes_per_pixel { row[index - bytes_per_pixel] } else { 0 };
            let up = previous[index];
            let prediction = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((left as u16 + up as u16) / 2) as u8,
                4 => paeth(left, up, if index >= bytes_per_pixel { previous[index - bytes_per_pixel] } else { 0 }),
                other => return Err(format!("row filter {other} is not supported")),
            };
            row[index] = row[index].wrapping_add(prediction);
        }

        out.extend_from_slice(&row);
        previous = row;
    }
    Ok(out)
}

/// PNG Paeth predictor, choosing the neighbouring byte closest to `left + up - upper_left`.
fn paeth(left: u8, up: u8, upper_left: u8) -> u8 {
    let estimate = left as i16 + up as i16 - upper_left as i16;
    let (left_distance, up_distance, corner_distance) = (
        (estimate - left as i16).abs(),
        (estimate - up as i16).abs(),
        (estimate - upper_left as i16).abs(),
    );
    if left_distance <= up_distance && left_distance <= corner_distance {
        left
    }
    else if up_distance <= corner_distance {
        up
    }
    else {
        upper_left
    }
}

fn font_object(base_font: &str) -> Vec<u8> {
    format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} /Encoding /WinAnsiEncoding >>").into_bytes()
}

fn stream_object(dictionary: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

fn loss(slide: &DeckSlide, element: &Element, code: &str, message: &str) -> Value {
    json!({
        "code": code,
        "severity": "warning",
        "slideId": slide.id.as_str(),
        "elementId": element.id.as_str(),
        "message": message,
    })
}

fn emu_to_pt(emu: i64) -> f64 {
    emu as f64 / EMU_PER_PT
}

fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 { "0".to_string() } else { format!("{rounded}") }
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Frame {
    fn from(transform: Transform) -> Self {
        Self {
            x: emu_to_pt(transform.x_emu),
            y: emu_to_pt(transform.y_emu),
            w: emu_to_pt(transform.width_emu),
            h: emu_to_pt(transform.height_emu),
        }
    }
}

/// One object table plus the cross-reference bookkeeping of a PDF file.
struct Pdf {
    objects: Vec<Option<Vec<u8>>>,
}

impl Pdf {
    fn new() -> Self {
        Self { objects: Vec::new() }
    }

    fn reserve(&mut self) -> usize {
        self.objects.push(None);
        self.objects.len()
    }

    fn put(&mut self, id: usize, body: Vec<u8>) {
        self.objects[id - 1] = Some(body);
    }

    fn finish(self, root: usize) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0usize; self.objects.len() + 1];
        for (index, object) in self.objects.iter().enumerate() {
            let id = index + 1;
            let Some(body) = object
            else {
                continue;
            };
            offsets[id] = out.len();
            out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            if !body.ends_with(b"\n") {
                out.push(b'\n');
            }
            out.extend_from_slice(b"endobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", self.objects.len() + 1).as_bytes());
        for offset in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root {root} 0 R >>\nstartxref\n{xref}\n%%EOF\n", self.objects.len() + 1).as_bytes(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use document_presentation::authored::{
        ChartData, ChartElementInput, ChartSeries, ChartType, Deck, DeckCreateOptions, ImageElementInput, ShapeElementInput,
        SlideAddOptions, TableCell, TableElementInput, TableRow, TextContent, TextElementInput, Transform, points_to_emu,
    };

    use super::*;

    fn count(haystack: &[u8], needle: &[u8]) -> usize {
        haystack.windows(needle.len()).filter(|window| *window == needle).count()
    }

    fn rich_deck() -> Deck {
        let mut deck = Deck::create(DeckCreateOptions { title: Some("PDF".to_string()), ..DeckCreateOptions::default() });
        deck.transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent::plain("Hello PDF\nSecond line"),
                    transform: Some(Transform::from_points(40.0, 40.0, 400.0, 100.0).unwrap()),
                    alt: Some("Greeting".to_string()),
                    placeholder: None,
                },
            )?;
            edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "ellipse".to_string(),
                    transform: Some(Transform::from_points(60.0, 200.0, 120.0, 80.0).unwrap()),
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
                        cells: vec![TableCell::plain("A"), TableCell::plain("B")],
                    }],
                    transform: Some(Transform::from_points(60.0, 320.0, 200.0, 24.0).unwrap()),
                    alt: Some("Grid".to_string()),
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
        .expect("PDF deck builds");
        deck
    }

    #[test]
    fn pdf_has_a_header_pages_and_a_cross_reference_table() {
        let deck = rich_deck();
        let (bytes, losses) = build(&deck).expect("PDF builds");
        assert!(bytes.starts_with(b"%PDF-1.7"), "the header must be PDF 1.7");
        assert_eq!(count(&bytes, b"/Type /Page "), 1, "one MediaBox page");
        assert_eq!(count(&bytes, b"/Type /Pages "), 1, "one page tree");
        assert_eq!(count(&bytes, b"/MediaBox [0 0 960 540]"), 1);
        assert!(count(&bytes, b"startxref") == 1);
        assert!(bytes.ends_with(b"%%EOF\n"));
        assert_eq!(count(&bytes, b"xref\n0 "), 1);
        assert!(losses.iter().any(|loss| loss["code"] == LOSS_CHART), "charts are reported: {losses:?}");
    }

    #[test]
    fn background_text_and_tables_are_painted_without_panic() {
        let mut deck = rich_deck();
        let mut document = deck.document().clone();
        document.slides[0].background = Some(Fill::Solid { color: "#1F2937".to_string() });
        deck = Deck::from_document(document).expect("valid deck");

        let (bytes, losses) = build(&deck).expect("PDF builds");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("0.12 0.16 0.22 rg"), "the background colour is painted");
        assert!(text.contains("(Hello PDF) Tj"), "text is drawn: {text}");
        assert!(text.contains("(Second line) Tj"));
        assert!(text.contains(" re S"), "table grid lines are drawn");
        assert!(!losses.iter().any(|loss| loss["code"] == "table"), "tables are painted, not dropped");
    }

    #[test]
    fn non_latin_text_is_reported_as_a_glyph_loss() {
        let mut deck = Deck::create(DeckCreateOptions::default());
        deck.transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent::plain("caf\u{e9} \u{4e2d}\u{6587}"),
                    transform: Some(Transform::from_points(40.0, 40.0, 400.0, 100.0).unwrap()),
                    alt: None,
                    placeholder: None,
                },
            )?;
            Ok(())
        })
        .expect("text deck builds");

        let (bytes, losses) = build(&deck).expect("PDF builds");
        assert_eq!(count(&bytes, b"(caf\xE9 ??) Tj"), 1, "Latin-1 is kept and wider text falls back");
        assert!(losses.iter().any(|loss| loss["code"] == LOSS_GLYPH), "glyph loss is reported");
    }

    /// IEEE CRC-32, the checksum every PNG chunk carries.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in bytes {
            crc ^= byte as u32;
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

    /// Smallest PNG encoder the tests need: 8-bit, filter 0, no interlacing.
    fn png(width: u32, height: u32, color_type: u8, rows: &[&[u8]]) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, color_type, 0, 0, 0]);
        out.extend_from_slice(&png_chunk(b"IHDR", &header));
        let mut raw = Vec::new();
        for row in rows {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        out.extend_from_slice(&png_chunk(b"IDAT", &Zlib::encode(&raw, DeflateLevel::Default)));
        out.extend_from_slice(&png_chunk(b"IEND", &[]));
        out
    }

    fn image_deck(bytes: Vec<u8>) -> Deck {
        let mut deck = Deck::create(DeckCreateOptions::default());
        deck.transaction(None, |edit| {
            let asset = edit.add_asset("picture.png", "image/png", bytes.clone(), None)?;
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_image(
                &slide,
                ImageElementInput {
                    asset,
                    transform: Some(Transform::from_points(20.0, 30.0, 100.0, 50.0).unwrap()),
                    alt: "Picture".to_string(),
                },
            )?;
            Ok(())
        })
        .expect("image deck builds");
        deck
    }

    #[test]
    fn png_pictures_become_flate_image_objects() {
        let rgb = png(2, 2, 2, &[&[255, 0, 0, 0, 255, 0], &[0, 0, 255, 255, 255, 255]]);
        let deck = image_deck(rgb);
        let (bytes, losses) = build(&deck).expect("PDF builds");
        let text = String::from_utf8_lossy(&bytes);

        assert!(losses.is_empty(), "a PNG picture is embedded, not dropped: {losses:?}");
        assert_eq!(count(&bytes, b"/Subtype /Image"), 1, "one image XObject");
        assert!(text.contains("/Width 2 /Height 2"), "the raster keeps its dimensions");
        assert!(text.contains("/ColorSpace /DeviceRGB"));
        assert!(text.contains("/BitsPerComponent 8"));
        assert!(text.contains("/Filter /FlateDecode"));
        assert!(!text.contains("/SMask"), "an opaque PNG needs no soft mask");
        assert!(text.contains("/Im1 Do"), "the picture is painted on the page");

        let raster = decode_png(&png(2, 2, 2, &[&[10, 20, 30, 40, 50, 60], &[70, 80, 90, 100, 110, 120]])).expect("decodes");
        assert_eq!(raster.samples, vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120]);
        assert!(raster.alpha.is_none());
    }

    #[test]
    fn png_transparency_becomes_a_soft_mask() {
        let rgba = png(2, 2, 6, &[&[255, 0, 0, 255, 0, 255, 0, 128], &[0, 0, 255, 0, 255, 255, 255, 255]]);
        let deck = image_deck(rgba);
        let (bytes, losses) = build(&deck).expect("PDF builds");
        let text = String::from_utf8_lossy(&bytes);

        assert!(losses.is_empty(), "an RGBA PNG is embedded, not dropped: {losses:?}");
        assert_eq!(count(&bytes, b"/Subtype /Image"), 2, "the image plus its soft mask");
        assert!(text.contains("/ColorSpace /DeviceRGB"));
        assert!(text.contains("/ColorSpace /DeviceGray"), "the mask is an 8-bit gray plane");
        assert!(text.contains("/SMask "), "the image references its mask");
    }

    #[test]
    fn unsupported_pictures_are_reported_as_a_loss() {
        let mut sixteen_bit = png(2, 2, 2, &[&[1, 2, 3, 4, 5, 6], &[7, 8, 9, 10, 11, 12]]);
        sixteen_bit[24] = 16;
        let deck = image_deck(sixteen_bit);
        let (bytes, losses) = build(&deck).expect("PDF builds");

        assert_eq!(count(&bytes, b"/Subtype /Image"), 0, "nothing fabricated for an unsupported PNG");
        assert!(losses.iter().any(|loss| loss["code"] == LOSS_IMAGE), "the skip is reported: {losses:?}");
    }

    #[test]
    fn row_filters_are_reversed() {
        let filtered = [0, 10, 20, 1, 5, 15, 2, 1, 2, 3, 2, 3, 4, 1, 1];
        let rows = unfilter(&filtered, 2, 5, 1).expect("filters reverse");
        assert_eq!(rows, vec![10, 20, 5, 20, 6, 22, 5, 16, 6, 17]);
    }

    #[test]
    fn error_policy_refuses_to_write_a_lossy_pdf() {
        let deck = rich_deck();
        let directory = std::env::temp_dir().join("open-ppt-pdf-policy");
        std::fs::create_dir_all(&directory).expect("temp directory");
        let target = directory.join("lossy.pdf");
        let error = export(
            &deck,
            &json!({ "format": "pdf", "target": target.display().to_string(), "overwrite": true, "lossPolicy": "error" }),
        )
        .expect_err("a lossy PDF must fail under the error policy");
        assert_eq!(error.code, "export/loss-exceeded");
        assert!(!target.exists(), "nothing is written when the error policy refuses");
    }
}
