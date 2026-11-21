//! Editable import of PresentationML packages.
//!
//! The upstream reader produces a format-native `PptxPresentation`; this module
//! maps it onto the authored deck model so an imported file can be edited,
//! rendered and exported through the same path as a natively created deck.
//!
//! Text, placeholders, pictures, preset shapes and tables are carried. Charts,
//! SmartArt, OLE objects, media and group shapes are skipped and reported with
//! the stable `import.pptx.*` codes the reader already collects. Shapes retain
//! their EMU placement, so nothing is re-measured on the way in.

use std::{fs, path::Path};

use document_formats::import::pptx::{
    Fill as PptxFill, IssueStatus, Line as PptxLine, PptxIssue, PptxPresentation, Shape as PptxShape, ShapeKind, TextFrame,
    import_pptx_bytes as read_presentation, model::codes,
};
use document_presentation::authored::{
    Deck, DeckCreateOptions, DeckEdit, DeckError, DeckSize, Fill, LayoutId, Line, PlaceholderRef, SlideAddOptions, SlideId,
    TableCell, TableElementInput, TableRow, TextAlign, TextContent, TextElementInput, TextParagraph, TextRun, Transform,
    points_to_emu,
};
use serde_json::{Value, json};

use crate::ServiceError;

/// Loss reported when a table cannot be represented as an editable table.
pub const LOSS_TABLE_EMPTY: &str = "import.pptx.table-empty";

/// Result of importing a PresentationML package as an editable deck.
pub struct ImportOutcome {
    /// Editable deck registered in the same store as created and opened decks.
    pub deck: Deck,
    /// Constructs the editable model does not carry, with stable codes.
    pub losses: Vec<Value>,
}

/// Reads one package and maps it onto an editable deck.
pub fn import_pptx(path: &Path) -> Result<ImportOutcome, ServiceError> {
    let bytes = fs::read(path)
        .map_err(|error| ServiceError::new("io/read", format!("could not read `{}`: {error}", path.display())))?;
    import_pptx_bytes(&path.display().to_string(), &bytes)
}

/// Maps an in-memory presentation package onto an editable deck.
pub fn import_pptx_bytes(label: &str, bytes: &[u8]) -> Result<ImportOutcome, ServiceError> {
    let presentation = read_presentation(label, bytes).map_err(crate::pptx::format_error)?;
    build(presentation)
}

fn build(presentation: PptxPresentation) -> Result<ImportOutcome, ServiceError> {
    if presentation.size.cx <= 0 || presentation.size.cy <= 0 {
        return Err(ServiceError::new("format/invalid-package", "the presentation slide size is missing or not positive"));
    }

    let mut deck = Deck::create(DeckCreateOptions {
        title: presentation.title.clone(),
        size: Some(DeckSize { width_emu: presentation.size.cx, height_emu: presentation.size.cy }),
        theme_name: None,
    });
    let mut losses = presentation.issues.iter().filter_map(issue_loss).collect::<Vec<_>>();

    deck.transaction(None, |edit| {
        let layout = edit.document().layouts.first().map(|layout| layout.id.clone());
        for slide in &presentation.slides {
            let slide_id = edit.add_slide(SlideAddOptions { layout: layout.clone(), index: None })?;
            if slide.hidden {
                edit.set_slide_hidden(&slide_id, true)?;
            }
            if let Some(notes) = slide.notes.as_deref().filter(|notes| !notes.trim().is_empty()) {
                edit.set_slide_notes(&slide_id, Some(notes.to_string()))?;
            }
            if slide.background.is_some() {
                losses.push(loss(
                    codes::FILL_APPROXIMATED,
                    Some(slide.index),
                    None,
                    "the editable slide model has no background slot, so the slide background is dropped",
                ));
            }
            for shape in &slide.shapes {
                import_shape(edit, &slide_id, slide.index, shape, layout.as_ref(), &mut losses)?;
            }
        }
        Ok(())
    })
    .map_err(ServiceError::from)?;

    Ok(ImportOutcome { deck, losses })
}

/// Codes the editable model carries even though the reader calls them partial.
const CARRIED_CODES: &[&str] = &[
    codes::NOTES,
    codes::HIDDEN_SLIDE,
    codes::PLACEHOLDER_INHERITANCE,
    codes::ROTATION,
    codes::TABLE_CELL_PARAGRAPHS,
    "import.pptx.partial-coverage",
];

fn issue_loss(issue: &PptxIssue) -> Option<Value> {
    if CARRIED_CODES.contains(&issue.code.as_str()) {
        return None;
    }
    let status = match issue.status {
        IssueStatus::Partial => "partial",
        IssueStatus::Unsupported => "unsupported",
    };
    Some(json!({
        "code": issue.code,
        "status": status,
        "slide": issue.slide,
        "shape": issue.shape,
        "message": issue.message,
    }))
}

fn import_shape(
    edit: &mut DeckEdit,
    slide: &SlideId,
    slide_index: u32,
    shape: &PptxShape,
    layout: Option<&LayoutId>,
    losses: &mut Vec<Value>,
) -> Result<(), DeckError> {
    match shape.kind {
        ShapeKind::Table => import_table(edit, slide, slide_index, shape, losses),
        ShapeKind::Picture => import_picture(edit, slide, shape, losses),
        ShapeKind::Shape => import_text_or_shape(edit, slide, slide_index, shape, layout, losses),
        // Charts, groups and other frames are refused by the reader, which has
        // already attached the matching `import.pptx.*` loss entry.
        ShapeKind::Chart | ShapeKind::Group | ShapeKind::Other => Ok(()),
    }
}

fn import_text_or_shape(
    edit: &mut DeckEdit,
    slide: &SlideId,
    slide_index: u32,
    shape: &PptxShape,
    layout: Option<&LayoutId>,
    losses: &mut Vec<Value>,
) -> Result<(), DeckError> {
    let has_text = shape.text.as_ref().is_some_and(|frame| !frame.is_empty());
    if has_text {
        let text = shape.text.as_ref().expect("checked above");
        edit.add_text(
            slide,
            TextElementInput {
                text: text_content(text),
                transform: authored_transform(shape.transform),
                alt: alt_for(shape),
                placeholder: placeholder(shape, layout),
            },
        )?;
        if shape.fill.as_ref().is_some_and(|fill| !matches!(fill, PptxFill::None)) || shape.line.is_some() {
            losses.push(loss(
                codes::FILL_APPROXIMATED,
                Some(slide_index),
                Some(shape.id),
                "a text box fill or outline has no slot on an editable text element and is dropped",
            ));
        }
        return Ok(());
    }

    if shape.geometry.is_some() || shape.fill.is_some() || shape.line.is_some() {
        edit.add_shape(
            slide,
            document_presentation::authored::ShapeElementInput {
                geometry: shape.geometry.clone().filter(|geometry| !geometry.is_empty()).unwrap_or_else(|| "rect".to_string()),
                transform: authored_transform(shape.transform),
                fill: authored_fill(shape.fill.as_ref()),
                line: authored_line(shape.line.as_ref()),
                alt: alt_for(shape),
            },
        )?;
    }
    Ok(())
}

fn import_picture(edit: &mut DeckEdit, slide: &SlideId, shape: &PptxShape, losses: &mut Vec<Value>) -> Result<(), DeckError> {
    let Some(image) = shape.image.as_ref()
    else {
        losses.push(loss(
            codes::VIEW_IMAGE_MISSING,
            None,
            Some(shape.id),
            "a picture carries no embedded relationship and is skipped",
        ));
        return Ok(());
    };
    let Some(bytes) = image.bytes.as_ref().filter(|bytes| !bytes.is_empty())
    else {
        losses.push(loss(
            codes::VIEW_IMAGE_MISSING,
            None,
            Some(shape.id),
            "a picture has no readable embedded bytes and is skipped",
        ));
        return Ok(());
    };

    let name = image.part.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or("image").to_string();
    let asset = match edit.document().assets.iter().find(|asset| asset.media_type == image.media_type && asset.bytes == *bytes)
    {
        Some(asset) => asset.id.clone(),
        None => edit.add_asset(name, image.media_type.clone(), bytes.clone(), None)?,
    };
    edit.add_image(
        slide,
        document_presentation::authored::ImageElementInput {
            asset,
            transform: authored_transform(shape.transform),
            alt: alt_for(shape).unwrap_or_else(|| "image".to_string()),
        },
    )?;
    Ok(())
}

fn import_table(
    edit: &mut DeckEdit,
    slide: &SlideId,
    slide_index: u32,
    shape: &PptxShape,
    losses: &mut Vec<Value>,
) -> Result<(), DeckError> {
    let Some(table) = shape.table.as_ref()
    else {
        return Ok(());
    };
    let mut columns = table.column_widths.len();
    for row in &table.rows {
        columns = columns.max(row.cells.len());
    }
    if columns == 0 || table.rows.is_empty() {
        losses.push(loss(
            LOSS_TABLE_EMPTY,
            Some(slide_index),
            Some(shape.id),
            "a table has no grid columns or no rows and is skipped",
        ));
        return Ok(());
    }

    let fallback_width = shape.transform.map(|transform| (transform.cx / columns as i64).max(1)).unwrap_or(914_400);
    let column_widths_emu = (0..columns)
        .map(|index| table.column_widths.get(index).copied().filter(|width| *width > 0).unwrap_or(fallback_width))
        .collect();

    let rows = table
        .rows
        .iter()
        .map(|row| {
            let mut cells = row
                .cells
                .iter()
                .map(|cell| TableCell {
                    text: text_content(&cell.text),
                    fill: authored_fill(cell.fill.as_ref()),
                    column_span: cell.colspan.max(1),
                    row_span: cell.rowspan.max(1),
                })
                .collect::<Vec<_>>();
            // The editable model stores one cell per grid column, so a merged
            // row is padded instead of being rejected.
            while cells.len() < columns {
                cells.push(TableCell::plain(""));
            }
            TableRow { height_emu: if row.height > 0 { row.height } else { 304_800 }, cells }
        })
        .collect::<Vec<_>>();

    edit.add_table(
        slide,
        TableElementInput { column_widths_emu, rows, transform: authored_transform(shape.transform), alt: alt_for(shape) },
    )?;
    Ok(())
}

fn placeholder(shape: &PptxShape, layout: Option<&LayoutId>) -> Option<PlaceholderRef> {
    let reference = shape.placeholder.as_ref()?;
    let name = if reference.kind.is_title() { "title".to_string() } else { reference.kind.as_str().to_string() };
    Some(PlaceholderRef { layout: layout.cloned(), name })
}

fn alt_for(shape: &PptxShape) -> Option<String> {
    let name = shape.name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn authored_transform(transform: Option<document_formats::import::pptx::Transform>) -> Option<Transform> {
    let transform = transform?;
    if transform.cx <= 0 || transform.cy <= 0 {
        return None;
    }
    Some(Transform {
        x_emu: transform.x,
        y_emu: transform.y,
        width_emu: transform.cx,
        height_emu: transform.cy,
        rotation_deg: (transform.rotation_deg.abs() > f64::EPSILON).then_some(transform.rotation_deg),
    })
}

fn authored_fill(fill: Option<&PptxFill>) -> Option<Fill> {
    match fill? {
        PptxFill::None => Some(Fill::None),
        PptxFill::Solid(color) => Some(Fill::Solid { color: color.clone() }),
    }
}

fn authored_line(line: Option<&PptxLine>) -> Option<Line> {
    let line = line?;
    let width_emu = if line.width_pt > 0.0 { points_to_emu(line.width_pt) } else { None };
    (line.color.is_some() || width_emu.is_some()).then(|| Line { color: line.color.clone(), width_emu })
}

fn text_content(frame: &TextFrame) -> TextContent {
    let paragraphs = frame
        .paragraphs
        .iter()
        .map(|paragraph| TextParagraph {
            runs: paragraph
                .runs
                .iter()
                .map(|run| TextRun {
                    text: run.text.clone(),
                    bold: run.style.bold.then_some(true),
                    italic: run.style.italic.then_some(true),
                    underline: run.style.underline.then_some(true),
                    strike: run.style.strike.then_some(true),
                    size_pt: run.style.size_pt.filter(|size| *size > 0.0),
                    color: run.style.color.clone(),
                    font_family: run.style.font.clone().filter(|font| !font.is_empty()),
                    hyperlink: run.hyperlink.clone(),
                })
                .collect(),
            level: paragraph.level.min(8),
            align: align(paragraph.align.as_deref()),
        })
        .collect::<Vec<_>>();
    if paragraphs.is_empty() { TextContent::plain("") } else { TextContent { paragraphs } }
}

fn align(value: Option<&str>) -> Option<TextAlign> {
    match value {
        Some("l") => Some(TextAlign::Left),
        Some("ctr") => Some(TextAlign::Center),
        Some("r") => Some(TextAlign::Right),
        Some("just") => Some(TextAlign::Justify),
        _ => None,
    }
}

fn loss(code: &str, slide: Option<u32>, shape: Option<u32>, message: &str) -> Value {
    json!({
        "code": code,
        "status": "partial",
        "slide": slide,
        "shape": shape,
        "message": message,
    })
}

#[cfg(test)]
mod tests {
    use document_presentation::authored::{
        ElementContent, ImageElementInput, ShapeElementInput, TableCell, TextElementInput, Transform,
    };

    use super::*;

    fn export_to_temp(deck: &Deck, name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join("open-ppt-import");
        std::fs::create_dir_all(&directory).expect("temp directory");
        let target = directory.join(name);
        crate::export::export(deck, &json!({ "format": "pptx", "target": target.display().to_string(), "overwrite": true }))
            .expect("package is written");
        target
    }

    #[test]
    fn an_exported_package_reimports_as_an_editable_deck() {
        let png = vec![137u8, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 0];
        let mut deck =
            Deck::create(DeckCreateOptions { title: Some("Import round trip".to_string()), ..DeckCreateOptions::default() });
        deck.transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.set_slide_title(&slide, "Quarterly review")?;
            edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent::plain("Hello import"),
                    transform: Some(Transform::from_points(60.0, 100.0, 400.0, 80.0).unwrap()),
                    alt: Some("Greeting".to_string()),
                    placeholder: None,
                },
            )?;
            let asset = edit.add_asset("dot.png", "image/png", png.clone(), None)?;
            edit.add_image(
                &slide,
                ImageElementInput {
                    asset,
                    transform: Some(Transform::from_points(60.0, 200.0, 40.0, 40.0).unwrap()),
                    alt: "Dot".to_string(),
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
            edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "ellipse".to_string(),
                    transform: Some(Transform::from_points(400.0, 200.0, 120.0, 80.0).unwrap()),
                    fill: Some(Fill::Solid { color: "#22AA44".to_string() }),
                    line: None,
                    alt: Some("Badge".to_string()),
                },
            )?;
            Ok(())
        })
        .expect("source deck builds");

        let target = export_to_temp(&deck, "round-trip.pptx");
        let outcome = import_pptx(&target).expect("the exported package imports");
        let imported = &outcome.deck;

        assert_eq!(imported.slides().len(), 1);
        assert_eq!(imported.title(), Some("Import round trip"));
        assert_eq!(imported.size(), deck.size());
        assert_eq!(imported.slides()[0].title(), "Quarterly review");
        assert_eq!(imported.assets().len(), 1);
        assert_eq!(imported.assets()[0].bytes, png);

        let table = imported.slides()[0]
            .elements
            .iter()
            .find_map(|element| match &element.content {
                ElementContent::Table(table) => Some(table),
                _ => None,
            })
            .expect("the table survives the round trip");
        assert_eq!(table.column_widths_emu.len(), 2);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(
            table.rows.iter().flat_map(|row| row.cells.iter()).map(|cell| cell.text.text()).collect::<Vec<_>>(),
            vec!["A", "B", "1", "2"]
        );

        let images = imported.slides()[0].elements.iter().filter(|element| element.kind.as_str() == "image").count();
        assert_eq!(images, 1);

        // Exporting the imported deck again must still produce a readable package.
        let second = export_to_temp(imported, "round-trip-again.pptx");
        let reimported = import_pptx(&second).expect("the re-exported package imports");
        assert_eq!(reimported.deck.slides().len(), 1);
        let table = reimported.deck.slides()[0]
            .elements
            .iter()
            .find_map(|element| match &element.content {
                ElementContent::Table(table) => Some(table),
                _ => None,
            })
            .expect("the table survives a second round trip");
        assert_eq!(table.rows[0].cells[0].text.text(), "A");
    }

    #[test]
    fn chart_frames_are_reported_as_losses_instead_of_failing() {
        let bytes = chart_package();
        let outcome = import_pptx_bytes("chart.pptx", &bytes).expect("chart packages still import");
        assert_eq!(outcome.deck.slides().len(), 1);
        assert!(outcome.deck.slides()[0].elements.is_empty(), "charts have no editable element");
        assert!(
            outcome.losses.iter().any(|loss| loss["code"] == codes::CHART),
            "the chart must be reported: {:?}",
            outcome.losses
        );
    }

    fn chart_package() -> Vec<u8> {
        const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/></Relationships>";
        const PRESENTATION: &str = "<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:sldIdLst><p:sldId id=\"256\" r:id=\"rId1\"/></p:sldIdLst><p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>";
        const PRESENTATION_RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide1.xml\"/></Relationships>";
        const SLIDE: &str = "<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"2\" name=\"Chart 1\"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"1000000\" y=\"1000000\"/><a:ext cx=\"4000000\" cy=\"3000000\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"rId1\"/></a:graphicData></a:graphic></p:graphicFrame></p:spTree></p:cSld></p:sld>";
        const SLIDE_RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart\" Target=\"../charts/chart1.xml\"/></Relationships>";

        let mut zip = data_binary_zip::ZipWriter::new();
        zip.push(data_binary_zip::ZipEntry::stored("_rels/.rels", RELS));
        zip.push(data_binary_zip::ZipEntry::stored("ppt/presentation.xml", PRESENTATION));
        zip.push(data_binary_zip::ZipEntry::stored("ppt/_rels/presentation.xml.rels", PRESENTATION_RELS));
        zip.push(data_binary_zip::ZipEntry::stored("ppt/slides/slide1.xml", SLIDE));
        zip.push(data_binary_zip::ZipEntry::stored("ppt/slides/_rels/slide1.xml.rels", SLIDE_RELS));
        zip.into_bytes().expect("chart package assembles")
    }
}
