//! Thin binding between OpenPPT and the upstream editable deck model.
//!
//! The deck model, transactions, project container, static playback and rule
//! checks live in the shared upstream tree. This crate only maps arguments,
//! files, PresentationML import and export, and JSON reports so the CLI and the
//! Node-API surface share one path instead of growing a second deck model.

#![deny(missing_docs)]

pub mod edit;
pub mod export;
pub mod import;
pub mod outline;
mod pdf;
pub mod pptx;
pub mod render;

pub use import::{ImportOutcome, import_pptx};

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use boost_fs::{OverwritePolicy, PublishOptions, publish_bytes_with_options};
use data_encoding::representation::Base64;
use document_presentation::authored::{
    Deck, DeckCreateOptions, DeckError, DeckSize, InspectOptions, Severity, SlideAddOptions, codes, decode_project, encode_project,
};
use serde_json::{Map, Value, json};

pub use document_presentation::authored::{Deck as EditableDeck, InspectCheck};

/// Encodes bytes as standard Base64 text.
pub(crate) fn base64_text(bytes: &[u8]) -> String {
    String::from_utf8(Base64::standard().encode(bytes)).expect("Base64 alphabet is ASCII")
}

/// Formats the product must classify, even while none is implemented.
const PRESENTATION_FORMATS: &[&str] = &[
    "pptx", "pptm", "ppsx", "ppsm", "potx", "potm", "thmx", "ppam", "ppa", "ppt", "pps", "pot", "xml", "odp", "pdf", "xps",
    "mp4", "wmv", "png", "jpg", "jpeg", "gif", "tif", "tiff",
];

/// Stable failure of one binding operation.
///
/// Construction goes through the typed constructors below: each one fixes a
/// stable code and attaches the parameters that code's contract promises, so
/// the binding forwards `code`, `params` and the original OS error instead of
/// re-deriving them from prose. The free-form pair is crate-internal only,
/// for forwarding upstream engine codes that already carry their own contract.
#[derive(Debug)]
pub struct ServiceError {
    code: String,
    message: String,
    params: Value,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl ServiceError {
    fn build(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_owned(), message: message.into(), params: Value::Null, source: None }
    }

    fn with_params(mut self, params: Value) -> Self {
        self.params = params;
        self
    }

    /// Adds one typed parameter, creating the payload object when the code carried none.
    pub(crate) fn with_param(mut self, key: &str, value: Value) -> Self {
        if !self.params.is_object() {
            self.params = Value::Object(Map::new());
        }
        self.params.as_object_mut().expect("object").insert(key.to_owned(), value);
        self
    }

    fn source(mut self, error: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(error));
        self
    }

    /// `session/argument-invalid`: a request parameter was rejected.
    pub fn argument(message: impl Into<String>) -> Self {
        let message = message.into();
        let reason = message.clone();
        Self::build("session/argument-invalid", message).with_params(json!({ "reason": reason }))
    }

    /// `object/not-found`: the addressed object is absent from the deck.
    pub fn not_found(kind: &str, id: &str) -> Self {
        Self::build("object/not-found", format!("`{kind}` `{id}` does not exist")).with_params(json!({ "kind": kind, "id": id }))
    }

    /// `io/read`: reading a file failed; the OS error kind is preserved.
    pub fn io_read(path: &Path, error: io::Error) -> Self {
        let kind = format!("{:?}", error.kind());
        Self::build("io/read", format!("could not read `{}`: {error}", path.display()))
            .with_params(json!({ "path": path.display().to_string(), "kind": kind }))
            .source(error)
    }

    /// `io/write`: publishing a file failed.
    pub fn io_write(path: &Path, reason: impl Into<String>) -> Self {
        Self::build("io/write", format!("could not publish `{}`: {}", path.display(), reason.into()))
            .with_params(json!({ "path": path.display().to_string() }))
    }

    /// `io/exists`: the target is present and overwrite was not granted.
    pub fn io_exists(path: &Path) -> Self {
        Self::build("io/exists", format!("`{}` already exists; pass overwrite to replace it", path.display()))
            .with_params(json!({ "path": path.display().to_string() }))
    }

    /// `format/unsupported`: the requested exchange format is not offered.
    pub fn format_unsupported(format: &str) -> Self {
        Self::build("format/unsupported", format!("`{format}` is not an offered export format")).with_params(json!({ "format": format }))
    }

    /// `format/invalid-package`: a package failed validation.
    pub fn format_invalid(message: impl Into<String>) -> Self {
        Self::build("format/invalid-package", message)
    }

    /// `export/loss-exceeded`: the loss policy refused the export.
    pub fn loss_exceeded(count: usize, policy: &str) -> Self {
        Self::build("export/loss-exceeded", format!("the export dropped {count} constructs and the loss policy is `{policy}`"))
            .with_params(json!({ "count": count, "policy": policy }))
    }

    /// Forwards a failure whose stable code already exists upstream.
    pub(crate) fn engine(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), params: Value::Null, source: None }
    }

    /// Stable, comparable code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Human-readable message; never carries binary payloads.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Typed per-code parameters, or `Value::Null` when the code carries none.
    pub fn params(&self) -> &Value {
        &self.params
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|error| error.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<DeckError> for ServiceError {
    fn from(error: DeckError) -> Self {
        let code = error.code();
        let mut params = Map::new();
        if code == codes::ARGUMENT_INVALID {
            // The catalog renders the rejection reason, so the upstream prose travels as a typed parameter.
            params.insert("reason".to_owned(), Value::from(error.message()));
        }
        if let Some(revision) = error.revision() {
            // A conflict contrasts the committed revision against the one the edit expected.
            let key = if code == codes::DECK_REVISION_CONFLICT { "actual" } else { "revision" };
            params.insert(key.to_owned(), Value::from(revision));
        }
        if code == codes::OBJECT_NOT_FOUND {
            // Not-found addresses the object as a kind/id pair, preferring the element.
            if let Some(element_id) = error.element_id() {
                params.insert("kind".to_owned(), Value::from("element"));
                params.insert("id".to_owned(), Value::from(element_id.as_str()));
            }
            else if let Some(slide_id) = error.slide_id() {
                params.insert("kind".to_owned(), Value::from("slide"));
                params.insert("id".to_owned(), Value::from(slide_id.as_str()));
            }
        }
        else {
            if let Some(slide_id) = error.slide_id() {
                params.insert("slideId".to_owned(), Value::from(slide_id.as_str()));
            }
            if let Some(element_id) = error.element_id() {
                params.insert("elementId".to_owned(), Value::from(element_id.as_str()));
            }
        }
        let mut mapped = Self::engine(code, error.message());
        if !params.is_empty() {
            mapped = mapped.with_params(Value::Object(params));
        }
        mapped
    }
}

impl From<io::Error> for ServiceError {
    fn from(error: io::Error) -> Self {
        let kind = format!("{:?}", error.kind());
        Self::build("io/failure", error.to_string()).with_params(json!({ "kind": kind })).source(error)
    }
}

/// What this build can actually do.
///
/// The matrix only claims what is wired: the native project container, editing,
/// saving, static playback, the implemented rule checks, PresentationML
/// playback, editable PresentationML import, PresentationML export, per-page
/// SVG rendering and a static PDF export. PPTX import, edit and export stay
/// `partial` because charts, SmartArt, OLE objects, media and groups are refused
/// or dropped with stable codes instead of being faked. PDF export is `partial`
/// because pictures other than JPEG and PNG, non-Latin glyphs, charts, formulas,
/// media and groups are reported rather than painted. PNG rendering, timed
/// builds, transitions and media playback stay `unsupported`.
pub fn capabilities() -> Value {
    let mut formats = Map::new();
    for format in PRESENTATION_FORMATS {
        let entry = if pptx::EXTENSIONS.contains(format) {
            json!({
                "import": "partial",
                "edit": "partial",
                "export": if matches!(*format, "pptx" | "ppsx" | "potx") { "partial" } else { "unsupported" },
                "playback": "partial",
                "lossCodes": pptx::LOSS_CODES,
            })
        }
        else {
            json!({
                "import": "unsupported",
                "edit": "unsupported",
                "export": "unsupported",
                "playback": "unsupported",
                "lossCodes": [],
            })
        };
        formats.insert((*format).to_string(), entry);
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
        "renderers": { "svg": "supported", "png": "unsupported", "pdf": "unsupported" },
        "exportTargets": { "pptx": "partial", "ppsx": "partial", "potx": "partial", "pdf": "partial", "odp": "unsupported" },
        "exportLossCodes": {
            "pptx": [export::LOSS_CHART, export::LOSS_FORMULA, export::LOSS_MEDIA, export::LOSS_GROUP, export::LOSS_IMAGE],
            "ppsx": [export::LOSS_CHART, export::LOSS_FORMULA, export::LOSS_MEDIA, export::LOSS_GROUP, export::LOSS_IMAGE],
            "potx": [export::LOSS_CHART, export::LOSS_FORMULA, export::LOSS_MEDIA, export::LOSS_GROUP, export::LOSS_IMAGE],
            "pdf": [pdf::LOSS_IMAGE, pdf::LOSS_GLYPH, pdf::LOSS_CHART, pdf::LOSS_FORMULA, pdf::LOSS_MEDIA, pdf::LOSS_GROUP],
            "odp": [],
        },
        "playback": { "staticSlides": "supported", "builds": "unsupported", "transitions": "unsupported", "media": "unsupported" },
    })
}

/// Requested slide extent: a named ratio or explicit points.
#[derive(Debug, Clone, PartialEq)]
pub enum SizeRequest {
    /// Named ratio such as `16:9`.
    Named(String),
    /// Explicit extent in points.
    Points {
        /// Slide width in points.
        width_pt: f64,
        /// Slide height in points.
        height_pt: f64,
    },
}

/// Arguments of [`create`].
#[derive(Debug, Clone, Default)]
pub struct CreateRequest {
    /// Deck title.
    pub title: Option<String>,
    /// Slide extent; absent keeps the engine default.
    pub size: Option<SizeRequest>,
    /// Theme name to start from.
    pub theme: Option<String>,
    /// Titles of the slides to create, in order.
    pub slides: Vec<String>,
}

/// Creates a deck with one title slide per requested title.
pub fn create(request: CreateRequest) -> Result<Deck, ServiceError> {
    let size = match &request.size {
        None => None,
        Some(SizeRequest::Named(name)) => Some(
            DeckSize::named(name)
                .ok_or_else(|| ServiceError::argument(format!("unknown slide size `{name}`")))?,
        ),
        Some(SizeRequest::Points { width_pt, height_pt }) => {
            Some(DeckSize::from_points(*width_pt, *height_pt).ok_or_else(|| {
                ServiceError::argument("custom slide size must use finite positive point values")
            })?)
        }
    };
    let mut deck = Deck::create(DeckCreateOptions { title: request.title, size, theme_name: request.theme });
    if !request.slides.is_empty() {
        let layout = deck
            .layouts()
            .first()
            .map(|layout| layout.id.clone())
            .ok_or_else(|| ServiceError::not_found("layout", "default"))?;
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
        .map_err(|error| ServiceError::io_read(path, error))?;
    decode_project(&bytes).map_err(ServiceError::from)
}

/// Writes a native project container through a temporary file and a rename.
///
/// The input file is never overwritten unless `overwrite` is set.
pub fn save(deck: &Deck, path: &Path, overwrite: bool) -> Result<usize, ServiceError> {
    let bytes = encode_project(deck).map_err(ServiceError::from)?;
    write_atomically(&bytes, path, overwrite)?;
    Ok(bytes.len())
}

/// Publishes bytes to `path` through a temporary file and a rename.
pub(crate) fn write_atomically(bytes: &[u8], path: &Path, overwrite: bool) -> Result<(), ServiceError> {
    if path.exists() && !overwrite {
        return Err(ServiceError::io_exists(path));
    }
    let options = PublishOptions {
        overwrite: if overwrite { OverwritePolicy::Replace } else { OverwritePolicy::RefuseExisting },
        ..PublishOptions::default()
    };
    publish_bytes_with_options(publish_target(path), bytes, options)
        .map_err(|error| ServiceError::io_write(path, error.to_string()))
}

/// Keeps a bare file name anchored to the working directory so the publisher
/// always has a folder to stage in.
fn publish_target(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => path.to_path_buf(),
        _ => PathBuf::from(".").join(path),
    }
}

/// Renders every slide to a standalone SVG document.
///
/// The report carries one page per slide (`slideId`, `index`, pixel size and
/// SVG) plus one loss entry per construct that has no SVG representation.
pub fn render(deck: &Deck) -> Result<Value, ServiceError> {
    render::render(deck)
}

/// Writes the deck as a PresentationML package or a static PDF and reports what
/// it dropped.
///
/// The request carries `format` (`pptx`, `ppsx`, `potx` or `pdf`), `target`,
/// `overwrite` and an optional `lossPolicy`.
pub fn export(deck: &Deck, request: &Value) -> Result<Value, ServiceError> {
    export::export(deck, request)
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
            let mut entry = json!({
                "code": issue.code,
                "severity": match issue.severity {
                    Severity::Info => "info",
                    Severity::Warning => "warning",
                    Severity::Error => "error",
                },
                "revision": issue.revision,
                "recoverable": issue.recoverable,
                "message": issue.message,
            });
            // Absent locators are omitted, not null, so the JSON matches the
            // optional `slideId` and `elementId` the TypeScript surface declares.
            if let Some(slide_id) = &issue.slide_id {
                entry["slideId"] = Value::from(slide_id.as_str());
            }
            if let Some(element_id) = &issue.element_id {
                entry["elementId"] = Value::from(element_id.as_str());
            }
            entry
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
                .ok_or_else(|| ServiceError::argument(format!("unknown check `{name}`")))
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
