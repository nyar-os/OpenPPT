//! PresentationML package writer.
//!
//! The writer emits one minimal but openable OPC package: presentation, one
//! slide master, one blank layout, one theme, the document properties and one
//! slide part per deck slide. Text, preset shapes and pictures are carried;
//! tables, charts, formulas and media are skipped and reported as loss codes.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use data_binary_zip::{ZipEntry, ZipWriter};
use document_presentation::authored::{Deck, DeckSlide, Element, ElementContent, Fill, Line, Transform};
use serde_json::{Value, json};

use crate::ServiceError;

/// Loss reported when a table is skipped.
pub const LOSS_TABLE: &str = "table";
/// Loss reported when a chart is skipped.
pub const LOSS_CHART: &str = "chart";
/// Loss reported when a formula is skipped.
pub const LOSS_FORMULA: &str = "formula";
/// Loss reported when audio or video is skipped.
pub const LOSS_MEDIA: &str = "media";
/// Loss reported when a group shape is skipped.
pub const LOSS_GROUP: &str = "group";
/// Loss reported when a picture references no readable embedded asset.
pub const LOSS_IMAGE: &str = "image";

/// Export targets backed by the same PresentationML package writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Editable presentation.
    Pptx,
    /// Slide show.
    Ppsx,
    /// Template.
    Potx,
}

impl Format {
    /// Parses one export target name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pptx" => Some(Self::Pptx),
            "ppsx" => Some(Self::Ppsx),
            "potx" => Some(Self::Potx),
            _ => None,
        }
    }

    /// Stable target name used in reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pptx => "pptx",
            Self::Ppsx => "ppsx",
            Self::Potx => "potx",
        }
    }

    fn main_content_type(self) -> &'static str {
        match self {
            Self::Pptx => "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
            Self::Ppsx => "application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml",
            Self::Potx => "application/vnd.openxmlformats-officedocument.presentationml.template.main+xml",
        }
    }
}

const XML_HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

const REL_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const REL_SLIDE_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const REL_SLIDE_LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_OFFICE_DOCUMENT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_CORE_PROPERTIES: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_EXTENDED_PROPERTIES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";

/// One slide part with its relationships (`rId` to package target).
struct SlidePart {
    xml: String,
    rels: Vec<(String, String, &'static str)>,
}

/// One embedded picture part.
struct MediaPart {
    file: String,
    content_type: String,
    bytes: Vec<u8>,
}

/// Writes the deck as a PresentationML package to `request.target`.
///
/// The request carries `format` (`pptx`, `ppsx` or `potx`), `target`,
/// `overwrite` and an optional `lossPolicy` (`warn` by default, `error` refuses
/// to write when anything would be dropped).
pub fn export(deck: &Deck, request: &Value) -> Result<Value, ServiceError> {
    let format = request.get("format").and_then(Value::as_str).unwrap_or("pptx");
    let format = Format::parse(format).ok_or_else(|| {
        ServiceError::new(
            "format/unsupported",
            format!("`{format}` is not a presentation export target; use pptx, ppsx or potx"),
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

    let (bytes, losses) = build(deck, format)?;
    if loss_policy == "error" && !losses.is_empty() {
        return Err(ServiceError::new(
            "export/loss-exceeded",
            format!("the {} projection would drop {} construct(s)", format.as_str(), losses.len()),
        ));
    }
    crate::write_atomically(&bytes, Path::new(target), overwrite)?;

    Ok(json!({
        "revision": deck.revision(),
        "format": format.as_str(),
        "target": target,
        "bytes": bytes.len(),
        "editable": true,
        "losses": losses,
        "diagnostics": [],
    }))
}

fn build(deck: &Deck, format: Format) -> Result<(Vec<u8>, Vec<Value>), ServiceError> {
    let mut media = Vec::new();
    let mut media_index: HashMap<String, usize> = HashMap::new();
    let mut losses = Vec::new();
    let mut slides = Vec::new();

    for slide in deck.slides() {
        let mut body = String::with_capacity(1024);
        let mut rels = Vec::new();
        let mut next_id = 2u32;
        for element in &slide.elements {
            match &element.content {
                ElementContent::Text(text) => {
                    write_text_shape(&mut body, next_id, element, text);
                    next_id += 1;
                }
                ElementContent::Shape(shape) => {
                    write_shape(&mut body, next_id, element, &shape.geometry, shape.fill.as_ref(), shape.line.as_ref());
                    next_id += 1;
                }
                ElementContent::Image(image) => {
                    let Some(asset) = deck.asset(&image.asset)
                    else {
                        losses.push(loss(slide, element, LOSS_IMAGE, "the referenced image asset is missing"));
                        continue;
                    };
                    if asset.bytes.is_empty() {
                        losses.push(loss(slide, element, LOSS_IMAGE, "the referenced image asset has no bytes"));
                        continue;
                    }
                    let index = match media_index.get(image.asset.as_str()) {
                        Some(index) => *index,
                        None => {
                            let (extension, content_type) = image_kind(&asset.media_type);
                            let index = media.len();
                            media.push(MediaPart {
                                file: format!("image{}.{}", index + 1, extension),
                                content_type: content_type.to_string(),
                                bytes: asset.bytes.clone(),
                            });
                            media_index.insert(image.asset.as_str().to_string(), index);
                            index
                        }
                    };
                    let relationship = format!("rId{}", rels.len() + 1);
                    rels.push((relationship.clone(), format!("../media/{}", media[index].file), REL_IMAGE));
                    write_picture(&mut body, next_id, element, &relationship);
                    next_id += 1;
                }
                ElementContent::Table(_) => {
                    losses.push(loss(
                        slide,
                        element,
                        LOSS_TABLE,
                        "tables are editable but are not written into this package yet",
                    ));
                }
                ElementContent::Chart(_) => {
                    losses.push(loss(
                        slide,
                        element,
                        LOSS_CHART,
                        "charts keep editable data that has no PresentationML chart part here",
                    ));
                }
                ElementContent::Formula(_) => {
                    losses.push(loss(
                        slide,
                        element,
                        LOSS_FORMULA,
                        "formulas keep a source expression that has no PresentationML part here",
                    ));
                }
                ElementContent::Media(_) => {
                    losses.push(loss(slide, element, LOSS_MEDIA, "audio and video parts are not written into this package"));
                }
            }
        }
        slides.push(SlidePart { xml: slide_xml(slide, &body), rels });
    }

    let size = deck.size();
    let presentation = presentation_xml(size.width_emu, size.height_emu, slides.len());
    let content_types = content_types_xml(format, slides.len(), &media);
    let mut zip = ZipWriter::new();
    zip.push(ZipEntry::stored("[Content_Types].xml", content_types));
    zip.push(ZipEntry::stored("_rels/.rels", root_rels_xml()));
    zip.push(ZipEntry::stored("docProps/core.xml", core_xml(deck)));
    zip.push(ZipEntry::stored("docProps/app.xml", app_xml(slides.len())));
    zip.push(ZipEntry::stored("ppt/presentation.xml", presentation));
    zip.push(ZipEntry::stored("ppt/_rels/presentation.xml.rels", presentation_rels_xml(slides.len())));
    zip.push(ZipEntry::stored("ppt/slideMasters/slideMaster1.xml", master_xml()));
    zip.push(ZipEntry::stored("ppt/slideMasters/_rels/slideMaster1.xml.rels", master_rels_xml()));
    zip.push(ZipEntry::stored("ppt/slideLayouts/slideLayout1.xml", layout_xml()));
    zip.push(ZipEntry::stored("ppt/slideLayouts/_rels/slideLayout1.xml.rels", layout_rels_xml()));
    zip.push(ZipEntry::stored("ppt/theme/theme1.xml", theme_xml()));
    for (index, slide) in slides.iter().enumerate() {
        zip.push(ZipEntry::stored(format!("ppt/slides/slide{}.xml", index + 1), slide.xml.as_str()));
        if !slide.rels.is_empty() {
            zip.push(ZipEntry::stored(format!("ppt/slides/_rels/slide{}.xml.rels", index + 1), slide_rels_xml(&slide.rels)));
        }
    }
    for part in &media {
        zip.push(ZipEntry::stored(format!("ppt/media/{}", part.file), part.bytes.clone()));
    }

    let bytes = zip.into_bytes().map_err(|error| {
        ServiceError::new("format/invalid-package", format!("could not assemble the PresentationML package: {error}"))
    })?;
    Ok((bytes, losses))
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

fn slide_xml(slide: &DeckSlide, body: &str) -> String {
    let mut xml = String::with_capacity(body.len() + 1024);
    xml.push_str(XML_HEADER);
    xml.push_str(&format!("<p:sld xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\""));
    if slide.hidden {
        xml.push_str(" show=\"0\"");
    }
    xml.push('>');
    xml.push_str("<p:cSld>");
    xml.push_str(&background_xml(slide));
    xml.push_str("<p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>");
    xml.push_str(body);
    xml.push_str("</p:spTree></p:cSld></p:sld>");
    xml
}

fn background_xml(slide: &DeckSlide) -> String {
    match &slide.background {
        Some(Fill::Solid { color }) => match color_hex(color) {
            Some(hex) => {
                format!("<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{hex}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>")
            }
            None => String::new(),
        },
        Some(Fill::None) => "<p:bg><p:bgPr><a:noFill/><a:effectLst/></p:bgPr></p:bg>".to_string(),
        Some(Fill::Inherit) | None => String::new(),
    }
}

fn write_text_shape(out: &mut String, shape_id: u32, element: &Element, text: &document_presentation::authored::TextContent) {
    out.push_str("<p:sp><p:nvSpPr>");
    write_non_visual(out, shape_id, "Text", element.alt.as_deref());
    out.push_str("<p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>");
    write_xfrm(out, element.transform);
    out.push_str("<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>");
    out.push_str(&fill_xml(element.style.fill.as_ref()));
    out.push_str(&line_xml(element.style.line.as_ref()));
    out.push_str("</p:spPr><p:txBody><a:bodyPr wrap=\"square\" anchor=\"t\"/><a:lstStyle/>");
    for paragraph in &text.paragraphs {
        out.push_str("<a:p>");
        let mut attributes = String::new();
        if paragraph.level > 0 {
            attributes.push_str(&format!(" lvl=\"{}\"", paragraph.level.min(8)));
        }
        if let Some(align) = paragraph.align {
            attributes.push_str(&format!(" algn=\"{}\"", align_value(align)));
        }
        if attributes.is_empty() {
            out.push_str("<a:pPr/>");
        }
        else {
            out.push_str(&format!("<a:pPr{attributes}/>"));
        }
        for run in &paragraph.runs {
            write_run(out, run);
        }
        out.push_str("<a:endParaRPr lang=\"en-US\"/></a:p>");
    }
    out.push_str("</p:txBody></p:sp>");
}

fn write_shape(out: &mut String, shape_id: u32, element: &Element, geometry: &str, fill: Option<&Fill>, line: Option<&Line>) {
    out.push_str("<p:sp><p:nvSpPr>");
    write_non_visual(out, shape_id, "Shape", element.alt.as_deref());
    out.push_str("<p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>");
    write_xfrm(out, element.transform);
    out.push_str(&format!("<a:prstGeom prst=\"{}\"><a:avLst/></a:prstGeom>", escape_attr(&preset_geometry(geometry))));
    out.push_str(&fill_xml(fill));
    out.push_str(&line_xml(line));
    out.push_str("</p:spPr></p:sp>");
}

fn write_picture(out: &mut String, shape_id: u32, element: &Element, relationship: &str) {
    out.push_str("<p:pic><p:nvPicPr>");
    write_non_visual(out, shape_id, "Picture", element.alt.as_deref());
    out.push_str("<p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>");
    out.push_str(&format!(
        "<p:blipFill><a:blip r:embed=\"{}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>",
        escape_attr(relationship)
    ));
    out.push_str("<p:spPr>");
    write_xfrm(out, element.transform);
    out.push_str("<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>");
}

fn write_non_visual(out: &mut String, shape_id: u32, kind: &str, alt: Option<&str>) {
    out.push_str(&format!("<p:cNvPr id=\"{shape_id}\" name=\"{kind} {shape_id}\""));
    if let Some(alt) = alt.filter(|value| !value.is_empty()) {
        out.push_str(&format!(" descr=\"{}\"", escape_attr(alt)));
    }
    out.push_str("/>");
}

fn write_xfrm(out: &mut String, transform: Transform) {
    out.push_str("<a:xfrm");
    if let Some(rotation) = transform.rotation_deg
        && rotation.abs() > f64::EPSILON
    {
        out.push_str(&format!(" rot=\"{}\"", (rotation * 60_000.0).round() as i64));
    }
    out.push('>');
    out.push_str(&format!(
        "<a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/>",
        transform.x_emu,
        transform.y_emu,
        transform.width_emu.max(0),
        transform.height_emu.max(0)
    ));
    out.push_str("</a:xfrm>");
}

fn write_run(out: &mut String, run: &document_presentation::authored::TextRun) {
    out.push_str("<a:r><a:rPr lang=\"en-US\" dirty=\"0\"");
    if let Some(size_pt) = run.size_pt {
        out.push_str(&format!(" sz=\"{}\"", (size_pt * 100.0).round() as i64));
    }
    if let Some(bold) = run.bold {
        out.push_str(if bold { " b=\"1\"" } else { " b=\"0\"" });
    }
    if let Some(italic) = run.italic {
        out.push_str(if italic { " i=\"1\"" } else { " i=\"0\"" });
    }
    if run.underline.unwrap_or(false) {
        out.push_str(" u=\"sng\"");
    }
    if run.strike.unwrap_or(false) {
        out.push_str(" strike=\"sngStrike\"");
    }
    out.push('>');
    if let Some(color) = run.color.as_deref().and_then(color_hex) {
        out.push_str(&format!("<a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill>"));
    }
    if let Some(family) = run.font_family.as_deref().filter(|value| !value.is_empty()) {
        out.push_str(&format!("<a:latin typeface=\"{}\"/>", escape_attr(family)));
    }
    out.push_str("</a:rPr>");
    out.push_str(&format!("<a:t>{}</a:t></a:r>", escape_text(&run.text)));
}

fn fill_xml(fill: Option<&Fill>) -> String {
    match fill {
        Some(Fill::Solid { color }) => match color_hex(color) {
            Some(hex) => format!("<a:solidFill><a:srgbClr val=\"{hex}\"/></a:solidFill>"),
            None => String::new(),
        },
        Some(Fill::None) => "<a:noFill/>".to_string(),
        Some(Fill::Inherit) | None => String::new(),
    }
}

fn line_xml(line: Option<&Line>) -> String {
    let Some(line) = line
    else {
        return String::new();
    };
    let width = line.width_emu.filter(|value| *value > 0).unwrap_or(12_700);
    let mut xml = format!("<a:ln w=\"{width}\">");
    if let Some(color) = line.color.as_deref().and_then(color_hex) {
        xml.push_str(&format!("<a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill>"));
    }
    xml.push_str("</a:ln>");
    xml
}

fn align_value(align: document_presentation::authored::TextAlign) -> &'static str {
    use document_presentation::authored::TextAlign;
    match align {
        TextAlign::Left => "l",
        TextAlign::Center => "ctr",
        TextAlign::Right => "r",
        TextAlign::Justify => "just",
    }
}

fn preset_geometry(value: &str) -> String {
    if !value.is_empty() && value.chars().all(|character| character.is_ascii_alphanumeric()) {
        value.to_string()
    }
    else {
        "rect".to_string()
    }
}

fn image_kind(media_type: &str) -> (&'static str, &'static str) {
    match media_type.to_ascii_lowercase().as_str() {
        "image/png" => ("png", "image/png"),
        "image/jpeg" | "image/jpg" => ("jpeg", "image/jpeg"),
        "image/gif" => ("gif", "image/gif"),
        "image/bmp" => ("bmp", "image/bmp"),
        "image/webp" => ("webp", "image/webp"),
        "image/svg+xml" => ("svg", "image/svg+xml"),
        "image/tiff" => ("tiff", "image/tiff"),
        "image/emf" => ("emf", "image/x-emf"),
        "image/wmf" => ("wmf", "image/x-wmf"),
        _ => ("bin", "application/octet-stream"),
    }
}

fn color_hex(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_start_matches('#');
    let expanded = match trimmed.len() {
        3 if trimmed.chars().all(|character| character.is_ascii_hexdigit()) => {
            trimmed.chars().flat_map(|character| [character, character]).collect::<String>()
        }
        6 | 8 => trimmed.to_string(),
        _ => return None,
    };
    let hex = if expanded.len() == 8 { expanded[..6].to_string() } else { expanded };
    hex.chars().all(|character| character.is_ascii_hexdigit()).then(|| hex.to_ascii_uppercase())
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

fn content_types_xml(format: Format, slide_count: usize, media: &[MediaPart]) -> String {
    let mut xml = String::from(XML_HEADER);
    xml.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    xml.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    xml.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    let mut defaults: BTreeMap<&str, &str> = BTreeMap::new();
    for part in media {
        let extension = part.file.rsplit_once('.').map(|(_, extension)| extension).unwrap_or("bin");
        defaults.entry(extension).or_insert(part.content_type.as_str());
    }
    for (extension, content_type) in defaults {
        xml.push_str(&format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            escape_attr(extension),
            escape_attr(content_type)
        ));
    }
    xml.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    xml.push_str("<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>");
    xml.push_str(&format!("<Override PartName=\"/ppt/presentation.xml\" ContentType=\"{}\"/>", format.main_content_type()));
    xml.push_str("<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>");
    xml.push_str("<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>");
    xml.push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    for index in 0..slide_count {
        xml.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>",
            index + 1
        ));
    }
    xml.push_str("</Types>");
    xml
}

fn root_rels_xml() -> String {
    format!(
        "{XML_HEADER}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_OFFICE_DOCUMENT}\" Target=\"ppt/presentation.xml\"/><Relationship Id=\"rId2\" Type=\"{REL_CORE_PROPERTIES}\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"{REL_EXTENDED_PROPERTIES}\" Target=\"docProps/app.xml\"/></Relationships>"
    )
}

fn presentation_xml(width_emu: i64, height_emu: i64, slide_count: usize) -> String {
    let mut xml = String::from(XML_HEADER);
    xml.push_str(&format!("<p:presentation xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\">"));
    xml.push_str("<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>");
    xml.push_str("<p:sldIdLst>");
    for index in 0..slide_count {
        xml.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 256 + index, index + 2));
    }
    xml.push_str("</p:sldIdLst>");
    xml.push_str(&format!("<p:sldSz cx=\"{width_emu}\" cy=\"{height_emu}\"/>"));
    xml.push_str("<p:notesSz cx=\"6858000\" cy=\"9144000\"/>");
    xml.push_str("</p:presentation>");
    xml
}

fn presentation_rels_xml(slide_count: usize) -> String {
    let mut xml = String::from(XML_HEADER);
    xml.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    xml.push_str(&format!("<Relationship Id=\"rId1\" Type=\"{REL_SLIDE_MASTER}\" Target=\"slideMasters/slideMaster1.xml\"/>"));
    for index in 0..slide_count {
        xml.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"{REL_SLIDE}\" Target=\"slides/slide{}.xml\"/>",
            index + 2,
            index + 1
        ));
    }
    xml.push_str("</Relationships>");
    xml
}

fn master_rels_xml() -> String {
    format!(
        "{XML_HEADER}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_SLIDE_LAYOUT}\" Target=\"../slideLayouts/slideLayout1.xml\"/><Relationship Id=\"rId2\" Type=\"{REL_THEME}\" Target=\"../theme/theme1.xml\"/></Relationships>"
    )
}

fn layout_rels_xml() -> String {
    format!(
        "{XML_HEADER}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_SLIDE_MASTER}\" Target=\"../slideMasters/slideMaster1.xml\"/></Relationships>"
    )
}

fn slide_rels_xml(rels: &[(String, String, &'static str)]) -> String {
    let mut xml = String::from(XML_HEADER);
    xml.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (id, target, kind) in rels {
        xml.push_str(&format!("<Relationship Id=\"{}\" Type=\"{kind}\" Target=\"{}\"/>", escape_attr(id), escape_attr(target)));
    }
    xml.push_str("</Relationships>");
    xml
}

fn master_xml() -> String {
    format!(
        "{XML_HEADER}<p:sldMaster xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\"><p:cSld name=\"OpenPPT Master\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr sz=\"3200\" b=\"1\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"Calibri Light\"/></a:defRPr></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:defRPr sz=\"1800\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"Calibri\"/></a:defRPr></a:lvl1pPr></p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr sz=\"1800\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"Calibri\"/></a:defRPr></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"
    )
}

fn layout_xml() -> String {
    format!(
        "{XML_HEADER}<p:sldLayout xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
    )
}

fn theme_xml() -> String {
    format!(
        "{XML_HEADER}<a:theme xmlns:a=\"{NS_A}\" name=\"OpenPPT\"><a:themeElements><a:clrScheme name=\"OpenPPT\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"1F2937\"/></a:dk2><a:lt2><a:srgbClr val=\"F3F4F6\"/></a:lt2><a:accent1><a:srgbClr val=\"2563EB\"/></a:accent1><a:accent2><a:srgbClr val=\"F97316\"/></a:accent2><a:accent3><a:srgbClr val=\"10B981\"/></a:accent3><a:accent4><a:srgbClr val=\"8B5CF6\"/></a:accent4><a:accent5><a:srgbClr val=\"EF4444\"/></a:accent5><a:accent6><a:srgbClr val=\"0EA5E9\"/></a:accent6><a:hlink><a:srgbClr val=\"2563EB\"/></a:hlink><a:folHlink><a:srgbClr val=\"7C3AED\"/></a:folHlink></a:clrScheme><a:fontScheme name=\"OpenPPT\"><a:majorFont><a:latin typeface=\"Calibri Light\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"OpenPPT\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"><a:tint val=\"50000\"/></a:schemeClr></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"><a:shade val=\"80000\"/></a:schemeClr></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w=\"9525\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/></a:ln><a:ln w=\"19050\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/></a:ln><a:ln w=\"28575\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"><a:tint val=\"95000\"/></a:schemeClr></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"><a:shade val=\"90000\"/></a:schemeClr></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"
    )
}

fn core_xml(deck: &Deck) -> String {
    let title = deck.title().unwrap_or("OpenPPT presentation");
    format!(
        "{XML_HEADER}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>OpenPPT</dc:creator><cp:lastModifiedBy>OpenPPT</cp:lastModifiedBy></cp:coreProperties>",
        escape_text(title)
    )
}

fn app_xml(slide_count: usize) -> String {
    format!(
        "{XML_HEADER}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>OpenPPT</Application><PresentationFormat>Widescreen</PresentationFormat><Slides>{slide_count}</Slides></Properties>"
    )
}

#[cfg(test)]
mod tests {
    use document_formats::import::pptx::{RenderOptions, import_pptx_bytes, render_presentation};
    use document_presentation::authored::{
        ChartData, ChartElementInput, ChartType, Deck, DeckCreateOptions, ShapeElementInput, SlideAddOptions, TableCell,
        TableElementInput, TableRow, TextContent, TextElementInput, Transform, points_to_emu,
    };

    use super::*;

    fn sample_deck() -> Deck {
        let mut deck =
            Deck::create(DeckCreateOptions { title: Some("Round trip".to_string()), ..DeckCreateOptions::default() });
        deck.transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.set_slide_title(&slide, "Quarterly review")?;
            edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent::plain("Hello <OpenPPT>"),
                    transform: Some(Transform::from_points(60.0, 100.0, 400.0, 80.0).unwrap()),
                    alt: Some("Greeting".to_string()),
                    placeholder: None,
                },
            )?;
            edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "ellipse".to_string(),
                    transform: Some(Transform::from_points(60.0, 200.0, 200.0, 120.0).unwrap()),
                    fill: Some(Fill::Solid { color: "#FF0000".to_string() }),
                    line: Some(Line { color: Some("#000000".to_string()), width_emu: points_to_emu(1.5) }),
                    alt: Some("Badge".to_string()),
                },
            )?;
            edit.add_table(
                &slide,
                TableElementInput {
                    column_widths_emu: vec![points_to_emu(100.0).unwrap(), points_to_emu(100.0).unwrap()],
                    rows: vec![
                        TableRow {
                            height_emu: points_to_emu(24.0).unwrap(),
                            cells: vec![TableCell::plain("A"), TableCell::plain("B")],
                        },
                        TableRow {
                            height_emu: points_to_emu(24.0).unwrap(),
                            cells: vec![TableCell::plain("1"), TableCell::plain("2")],
                        },
                    ],
                    transform: Some(Transform::from_points(60.0, 340.0, 200.0, 60.0).unwrap()),
                    alt: Some("Grid".to_string()),
                },
            )?;
            edit.add_chart(
                &slide,
                ChartElementInput {
                    chart_type: ChartType::Column,
                    data: ChartData {
                        categories: vec!["Q1".to_string(), "Q2".to_string()],
                        series: vec![document_presentation::authored::ChartSeries {
                            name: "Sales".to_string(),
                            values: vec![1.0, 2.0],
                        }],
                    },
                    transform: Some(Transform::from_points(300.0, 200.0, 300.0, 180.0).unwrap()),
                    alt: "Sales".to_string(),
                },
            )?;
            Ok(())
        })
        .expect("sample deck builds");
        deck
    }

    #[test]
    fn pptx_round_trips_slides_and_text() {
        let deck = sample_deck();
        let (bytes, losses) = build(&deck, Format::Pptx).expect("package builds");
        assert!(bytes.starts_with(b"PK\x03\x04"), "writer must emit a ZIP container");
        assert!(losses.iter().any(|loss| loss["code"] == LOSS_TABLE));
        assert!(losses.iter().any(|loss| loss["code"] == LOSS_CHART));

        let imported = import_pptx_bytes("round-trip.pptx", &bytes).expect("the written package imports");
        assert_eq!(imported.slides.len(), deck.slides().len());
        let text: Vec<String> = imported
            .slides
            .iter()
            .flat_map(|slide| slide.shapes.iter())
            .filter_map(|shape| shape.text.as_ref())
            .map(|frame| frame.text())
            .collect();
        assert!(text.iter().any(|value| value.contains("Hello <OpenPPT>")), "slide text must survive the round trip: {text:?}");
        assert!(text.iter().any(|value| value.contains("Quarterly review")), "title text must survive the round trip");

        let render = render_presentation(&imported, &RenderOptions::default());
        assert_eq!(render.views.len(), deck.slides().len());
    }

    #[test]
    fn pictures_are_embedded_into_the_package() {
        let mut deck = Deck::create(DeckCreateOptions::default());
        let png = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 0];
        deck.transaction(None, |edit| {
            let asset = edit.add_asset("dot.png", "image/png", png.clone(), None)?;
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_image(
                &slide,
                document_presentation::authored::ImageElementInput {
                    asset,
                    transform: Some(Transform::from_points(10.0, 10.0, 40.0, 40.0).unwrap()),
                    alt: "Dot".to_string(),
                },
            )?;
            Ok(())
        })
        .expect("image deck builds");

        let (bytes, losses) = build(&deck, Format::Pptx).expect("package builds");
        assert!(losses.is_empty());
        let imported = import_pptx_bytes("image.pptx", &bytes).expect("the written package imports");
        let image = imported.slides[0].shapes.iter().find_map(|shape| shape.image.as_ref()).expect("picture is carried");
        assert_eq!(image.bytes.as_deref(), Some(png.as_slice()));
    }

    #[test]
    fn export_refuses_loss_under_error_policy() {
        let deck = sample_deck();
        let directory = std::env::temp_dir().join("open-ppt-export-policy");
        std::fs::create_dir_all(&directory).expect("temp directory");
        let target = directory.join("lossy.pptx");
        let error = export(
            &deck,
            &json!({ "format": "pptx", "target": target.display().to_string(), "overwrite": true, "lossPolicy": "error" }),
        )
        .expect_err("a lossy projection must fail under the error policy");
        assert_eq!(error.code, "export/loss-exceeded");
        assert!(!target.exists(), "nothing is written when the error policy refuses");
    }
}
