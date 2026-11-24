//! Playback of PresentationML `.pptx` packages.
//!
//! The importer and the fixed-canvas slide view live upstream in
//! `document-formats`; this module only reads the package once, renders every
//! visible slide to an SVG canvas and keeps the result so the CLI and the
//! window can step through slides without re-parsing the file.

use std::{fs, path::Path};

use document_formats::{
    FormatError,
    import::pptx::{IssueStatus, PptxIssue, RenderOptions, import_pptx_bytes, model::codes, render_presentation},
};
use serde_json::{Value, json};

use crate::ServiceError;

/// Formats whose package this reader can play.
///
/// They share one PresentationML reader; only macro storage and the intended
/// show/template use differ, and macros are never executed.
pub const EXTENSIONS: &[&str] = &["pptx", "pptm", "ppsx", "ppsm", "potx", "potm"];

/// Refusal codes reported when a construct is only partially carried.
///
/// The reader owns the `import.pptx.*` codes; `table-empty` and
/// `fill-approximated` are reported by the editable importer for constructs the
/// authored model cannot represent.
pub const LOSS_CODES: &[&str] = &[
    codes::CHART,
    codes::SMART_ART,
    codes::OLE_OBJECT,
    codes::MEDIA,
    codes::GROUP,
    codes::FILL_APPROXIMATED,
    crate::import::LOSS_TABLE_EMPTY,
];

/// One imported presentation at its rendered state.
///
/// The source model is dropped after rendering: playback only needs the canvas
/// size, the painted slides and the collected issues.
pub struct Playback {
    label: String,
    title: Option<String>,
    size: document_formats::import::pptx::SlideSize,
    views: Vec<document_formats::import::pptx::SlideView>,
    issues: Vec<PptxIssue>,
}

impl Playback {
    /// Compact description of the show used by the CLI and the window.
    pub fn summary(&self) -> Value {
        let slides = self
            .views
            .iter()
            .map(|view| {
                json!({
                    "index": view.index,
                    "widthPx": view.width_px,
                    "heightPx": view.height_px,
                })
            })
            .collect::<Vec<_>>();

        json!({
            "label": self.label,
            "title": self.title,
            "size": {
                "widthPx": self.size.width_px(),
                "heightPx": self.size.height_px(),
                "widthPt": self.size.width_pt(),
                "heightPt": self.size.height_pt(),
            },
            "slides": slides,
            "issues": self.issues.iter().map(issue_value).collect::<Vec<_>>(),
        })
    }

    /// One painted slide by its one-based number.
    pub fn slide(&self, index: u32) -> Result<Value, ServiceError> {
        let view =
            self.views.iter().find(|view| view.index == index).ok_or_else(|| {
                ServiceError::not_found("played slide", &index.to_string())
            })?;
        Ok(json!({
            "index": view.index,
            "widthPx": view.width_px,
            "heightPx": view.height_px,
            "svg": view.svg,
        }))
    }
}

/// Reads one package and renders every visible slide.
pub fn open(path: &Path) -> Result<Playback, ServiceError> {
    let bytes = fs::read(path)
        .map_err(|error| ServiceError::io_read(path, error))?;
    let presentation = import_pptx_bytes(&path.display().to_string(), &bytes).map_err(format_error)?;
    let render = render_presentation(&presentation, &RenderOptions::default());
    let mut issues = presentation.issues;
    issues.extend(render.issues);
    Ok(Playback { label: presentation.label, title: presentation.title, size: presentation.size, views: render.views, issues })
}

fn issue_value(issue: &PptxIssue) -> Value {
    json!({
        "code": issue.code,
        "status": match issue.status {
            IssueStatus::Partial => "partial",
            IssueStatus::Unsupported => "unsupported",
        },
        "message": issue.message,
        "slide": issue.slide,
        "shape": issue.shape,
    })
}

pub(crate) fn format_error(error: FormatError) -> ServiceError {
    let code = match &error {
        FormatError::NotImplemented { .. } => "capability/not-implemented",
        FormatError::InvalidInput { .. } | FormatError::Parse { .. } => "format/invalid-package",
        FormatError::Unsupported { .. } => "format/unsupported",
        FormatError::Budget { .. } => "session/budget-exceeded",
    };
    ServiceError::engine(code, error.to_string())
}
