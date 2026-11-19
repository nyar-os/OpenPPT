//! Thin binding between OpenPPT and the upstream editable deck model.
//!
//! The deck model, transactions, project container, static playback and rule
//! checks live in the shared upstream tree. This crate only maps arguments,
//! files and JSON reports so the CLI and the Node-API surface share one path
//! instead of growing a second deck model.

#![deny(missing_docs)]

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use document_presentation::authored::{
    Deck, DeckCreateOptions, DeckError, DeckSize, InspectOptions, Severity, SlideAddOptions, decode_project, encode_project,
};
use serde_json::{Map, Value, json};

pub use document_presentation::authored::{Deck as EditableDeck, InspectCheck};

/// Formats the product must classify, even while none is implemented.
const PRESENTATION_FORMATS: &[&str] = &[
    "pptx", "pptm", "ppsx", "ppsm", "potx", "potm", "thmx", "ppam", "ppa", "ppt", "pps", "pot", "xml", "odp", "pdf", "xps",
    "mp4", "wmv", "png", "jpg", "jpeg", "gif", "tif", "tiff",
];

/// Stable failure of one binding operation.
#[derive(Debug)]
pub struct ServiceError {
    /// Stable, comparable code.
    pub code: String,
    /// Human-readable message; never carries binary payloads.
    pub message: String,
}

impl ServiceError {
    /// Builds a failure from a stable code and a message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServiceError {}

impl From<DeckError> for ServiceError {
    fn from(error: DeckError) -> Self {
        Self::new(error.code(), error.message())
    }
}

impl From<io::Error> for ServiceError {
    fn from(error: io::Error) -> Self {
        Self::new("io/failure", error.to_string())
    }
}

/// What this build can actually do.
///
/// The matrix only claims what is wired: the native project container, editing,
/// saving, static playback and the implemented rule checks. PPTX exchange,
/// raster rendering and timed playback stay `unsupported` until they land.
pub fn capabilities() -> Value {
    let mut formats = Map::new();
    for format in PRESENTATION_FORMATS {
        formats.insert(
            (*format).to_string(),
            json!({
                "import": "unsupported",
                "edit": "unsupported",
                "export": "unsupported",
                "playback": "unsupported",
                "lossCodes": [],
            }),
        );
    }

    json!({
        "engine": { "package": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION") },
        "project": { "import": "supported", "edit": "supported", "export": "supported", "playback": "supported" },
        "formats": Value::Object(formats),
        "checks": {
            "overflow": "unsupported",
            "contrast": "supported",
            "alignment": "unsupported",
            "assets": "supported",
            "fonts": "unsupported",
            "out-of-bounds": "supported",
        },
        "renderers": { "svg": "unsupported", "png": "unsupported", "pdf": "unsupported" },
        "exportTargets": { "pptx": "unsupported", "ppsx": "unsupported", "potx": "unsupported", "pdf": "unsupported", "odp": "unsupported" },
        "playback": { "staticSlides": "supported", "builds": "unsupported", "transitions": "unsupported", "media": "unsupported" },
    })
}

/// Arguments of [`create`].
#[derive(Debug, Clone, Default)]
pub struct CreateRequest {
    /// Deck title.
    pub title: Option<String>,
    /// Named slide extent such as `16:9`.
    pub size: Option<String>,
    /// Titles of the slides to create, in order.
    pub slides: Vec<String>,
}

/// Creates a deck with one title slide per requested title.
pub fn create(request: CreateRequest) -> Result<Deck, ServiceError> {
    let size = match request.size.as_deref() {
        None => None,
        Some(name) => Some(
            DeckSize::named(name)
                .ok_or_else(|| ServiceError::new("session/argument-invalid", format!("unknown slide size `{name}`")))?,
        ),
    };
    let mut deck = Deck::create(DeckCreateOptions { title: request.title, size, theme_name: None });
    if !request.slides.is_empty() {
        let layout = deck
            .layouts()
            .first()
            .map(|layout| layout.id.clone())
            .ok_or_else(|| ServiceError::new("object/not-found", "the default deck has no layout"))?;
        deck.transaction(None, |edit| {
            for title in &request.slides {
                let slide = edit.add_slide(SlideAddOptions { layout: Some(layout.clone()), index: None })?;
                edit.set_slide_title(&slide, title.clone())?;
            }
            Ok(())
        })?;
    }
    Ok(deck)
}

/// Reads a native project container.
pub fn open(path: &Path) -> Result<Deck, ServiceError> {
    let bytes = fs::read(path)
        .map_err(|error| ServiceError::new("io/read", format!("could not read `{}`: {error}", path.display())))?;
    decode_project(&bytes).map_err(ServiceError::from)
}

/// Writes a native project container through a temporary file and a rename.
///
/// The input file is never overwritten unless `overwrite` is set.
pub fn save(deck: &Deck, path: &Path, overwrite: bool) -> Result<(), ServiceError> {
    if path.exists() && !overwrite {
        return Err(ServiceError::new(
            "io/exists",
            format!("`{}` already exists; pass overwrite to replace it", path.display()),
        ));
    }
    let bytes = encode_project(deck).map_err(ServiceError::from)?;
    let temp = temporary_path(path);
    fs::write(&temp, &bytes)
        .map_err(|error| ServiceError::new("io/write", format!("could not write `{}`: {error}", temp.display())))?;
    if overwrite && path.exists() {
        fs::remove_file(path)
            .map_err(|error| ServiceError::new("io/write", format!("could not replace `{}`: {error}", path.display())))?;
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(ServiceError::new("io/write", format!("could not publish `{}`: {error}", path.display())));
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Summary of one deck at its committed revision.
pub fn summary(deck: &Deck) -> Value {
    let slides = deck
        .slides()
        .iter()
        .enumerate()
        .map(|(index, slide)| {
            json!({
                "index": index,
                "id": slide.id.as_str(),
                "title": slide.title(),
                "hidden": slide.hidden,
                "elements": slide.elements.len(),
                "notes": slide.notes,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "id": deck.id().as_str(),
        "revision": deck.revision(),
        "title": deck.document().title,
        "size": { "widthPt": deck.document().size.width_pt(), "heightPt": deck.document().size.height_pt() },
        "layouts": deck.layouts().iter().map(|layout| json!({ "id": layout.id.as_str(), "name": layout.name })).collect::<Vec<_>>(),
        "slides": slides,
    })
}

/// Runs the implemented rule checks and reports every finding with its location.
pub fn inspect(deck: &Deck, checks: Option<Vec<InspectCheck>>) -> Value {
    let report = deck.inspect(InspectOptions { checks });
    let issues = report
        .issues
        .iter()
        .map(|issue| {
            json!({
                "code": issue.code,
                "severity": match issue.severity {
                    Severity::Info => "info",
                    Severity::Warning => "warning",
                    Severity::Error => "error",
                },
                "revision": issue.revision,
                "slideId": issue.slide_id.as_ref().map(|id| id.as_str()),
                "elementId": issue.element_id.as_ref().map(|id| id.as_str()),
                "recoverable": issue.recoverable,
                "message": issue.message,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "revision": report.revision,
        "checkerVersion": report.checker_version,
        "passed": report.passed,
        "issues": issues,
    })
}

/// Parses the comma-separated check list used by the CLI.
pub fn parse_checks(value: &str) -> Result<Vec<InspectCheck>, ServiceError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            InspectCheck::parse(name)
                .ok_or_else(|| ServiceError::new("session/argument-invalid", format!("unknown check `{name}`")))
        })
        .collect()
}

/// Frames of a static playback run, in show order.
pub fn play(deck: &Deck) -> Result<Vec<Value>, ServiceError> {
    let mut session = deck.playback(None).map_err(ServiceError::from)?;
    session.start().map_err(ServiceError::from)?;
    let mut frames = vec![frame(&session)];
    loop {
        let before = session.state().slide_id.clone();
        session.next().map_err(ServiceError::from)?;
        let state = session.state();
        if !state.playing || state.slide_id == before {
            break;
        }
        frames.push(frame(&session));
    }
    Ok(frames)
}

fn frame(session: &document_presentation::authored::PlaybackSession) -> Value {
    let state = session.state();
    json!({
        "revision": state.revision,
        "slideId": state.slide_id.as_ref().map(|id| id.as_str()),
        "buildStep": state.build_step,
        "timeMs": state.time_ms,
        "notes": session.notes(),
    })
}
