//! Thin Node-API surface for OpenPPT.
//!
//! Every export does one orthogonal operation and returns a JSON string, so the
//! TypeScript layer owns argument parsing, help text and exit codes through
//! `@wae/commander`. Deck semantics, saving and playback stay upstream; this
//! layer only keeps deck handles and maps values.
//!
//! Functions that are still unwired return the stable
//! `capability/not-implemented` code instead of a fabricated deck or report.

#![deny(missing_docs)]

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use open_ppt_deck::{CreateRequest, EditableDeck as Deck, InspectCheck, ServiceError, SizeRequest, pptx::Playback};
use serde_json::{Value, json};
use wae::{
    CallContext, Error, JsValue, Result, function, optional_string_prop, register_module,
    webview::{IpcHandler, IpcOutcome, IpcSink, OpenOptions, WebViewError, WebViewHost},
};

/// Live deck handles owned by this process.
#[derive(Default)]
struct DeckHandles {
    next: u64,
    decks: HashMap<u64, Deck>,
}

fn handles() -> &'static Mutex<DeckHandles> {
    static HANDLES: OnceLock<Mutex<DeckHandles>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(DeckHandles::default()))
}

fn lock() -> std::sync::MutexGuard<'static, DeckHandles> {
    handles().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn insert(deck: Deck) -> u64 {
    let mut guard = lock();
    guard.next += 1;
    let handle = guard.next;
    guard.decks.insert(handle, deck);
    handle
}

fn with_deck<T>(handle: u64, work: impl FnOnce(&Deck) -> T) -> Result<T> {
    let guard = lock();
    guard.decks.get(&handle).map(work).ok_or_else(|| Error::new("deck/not-found: the deck handle is not open"))
}

fn with_deck_mut<T>(handle: u64, work: impl FnOnce(&mut Deck) -> T) -> Result<T> {
    let mut guard = lock();
    guard.decks.get_mut(&handle).map(work).ok_or_else(|| Error::new("deck/not-found: the deck handle is not open"))
}

fn remove(handle: u64) -> bool {
    lock().decks.remove(&handle).is_some()
}

/// Live PresentationML playback handles, kept apart from editable decks.
#[derive(Default)]
struct PlaybackHandles {
    next: u64,
    items: HashMap<u64, Playback>,
}

fn playbacks() -> &'static Mutex<PlaybackHandles> {
    static HANDLES: OnceLock<Mutex<PlaybackHandles>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(PlaybackHandles::default()))
}

fn playback_lock() -> std::sync::MutexGuard<'static, PlaybackHandles> {
    playbacks().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn insert_playback(playback: Playback) -> u64 {
    let mut guard = playback_lock();
    guard.next += 1;
    let handle = guard.next;
    guard.items.insert(handle, playback);
    handle
}

fn with_playback<T>(handle: u64, work: impl FnOnce(&Playback) -> T) -> Result<T> {
    let guard = playback_lock();
    guard.items.get(&handle).map(work).ok_or_else(|| Error::new("object/not-found: the presentation handle is not open"))
}

fn remove_playback(handle: u64) -> bool {
    playback_lock().items.remove(&handle).is_some()
}

fn fail(error: ServiceError) -> Error {
    Error::new(format!("{}: {}", error.code, error.message))
}

fn argument(ctx: &CallContext, index: usize) -> Result<JsValue> {
    ctx.args().get(index).copied().ok_or_else(|| Error::new("session/argument-invalid: a required argument is missing"))
}

fn text(ctx: &CallContext, index: usize) -> Result<String> {
    argument(ctx, index)?.as_string()
}

/// Reads an optional JSON request argument; an absent value becomes `null`.
fn optional_json(ctx: &CallContext, index: usize) -> Result<Value> {
    match ctx.args().get(index) {
        Some(value) if !value.is_undefined()? => {
            let raw = value.as_string()?;
            serde_json::from_str(&raw).map_err(|error| Error::new(format!("session/argument-invalid: {error}")))
        }
        _ => Ok(Value::Null),
    }
}

/// Rejects render formats this build cannot paint.
fn check_render_format(request: &Value) -> Result<()> {
    match request.get("format").and_then(Value::as_str) {
        None | Some("svg") => Ok(()),
        Some(other) => Err(Error::new(format!("capability/not-implemented: the `{other}` renderer is not implemented yet"))),
    }
}

fn handle(ctx: &CallContext, index: usize) -> Result<u64> {
    let raw = argument(ctx, index)?.as_i64()?;
    u64::try_from(raw).map_err(|_| Error::new("session/argument-invalid: the deck handle must be a non-negative integer"))
}

fn parse_size(request: &Value) -> Result<Option<SizeRequest>> {
    match &request["size"] {
        Value::Null => Ok(None),
        Value::String(name) => Ok(Some(SizeRequest::Named(name.clone()))),
        Value::Object(map) => {
            let number = |key: &str| {
                map.get(key)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| Error::new(format!("session/argument-invalid: `size.{key}` must be a number in points")))
            };
            Ok(Some(SizeRequest::Points { width_pt: number("widthPt")?, height_pt: number("heightPt")? }))
        }
        _ => Err(Error::new("session/argument-invalid: `size` must be a named ratio or `{ widthPt, heightPt }`")),
    }
}

fn respond(ctx: &CallContext, value: &Value) -> Result<JsValue> {
    let text = value.to_string();
    ctx.env().string(&text)
}

/// Library version of the native binding.
#[function]
fn version(ctx: CallContext) -> Result<JsValue> {
    ctx.env().string(env!("CARGO_PKG_VERSION"))
}

/// Reports formats, checks, renderers and playback support of the engine.
///
/// The matrix comes from the same binding crate the CLI uses, so neither entry
/// point can report different capabilities.
#[function]
fn capabilities(ctx: CallContext) -> Result<JsValue> {
    let matrix = open_ppt_deck::capabilities().to_string();
    ctx.env().string(&matrix)
}

/// Creates a deck from a JSON request and returns `{ handle, deck }`.
#[function]
fn deck_create(ctx: CallContext) -> Result<JsValue> {
    let raw = text(&ctx, 0)?;
    let request: Value =
        serde_json::from_str(&raw).map_err(|error| Error::new(format!("session/argument-invalid: {error}")))?;
    let slides = request["slides"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let deck = open_ppt_deck::create(CreateRequest {
        title: request["title"].as_str().map(str::to_string),
        size: parse_size(&request)?,
        theme: request["theme"].as_str().map(str::to_string),
        slides,
    })
    .map_err(fail)?;
    let summary = open_ppt_deck::summary(&deck);
    let handle = insert(deck);
    respond(&ctx, &json!({ "handle": handle, "deck": summary }))
}

/// Opens a native project from a path and returns `{ handle, deck }`.
#[function]
fn deck_open(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let deck = open_ppt_deck::open(std::path::Path::new(&path)).map_err(fail)?;
    let summary = open_ppt_deck::summary(&deck);
    let handle = insert(deck);
    respond(&ctx, &json!({ "handle": handle, "deck": summary }))
}

/// Returns the summary of one open deck.
#[function]
fn deck_summary(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let summary = with_deck(handle, open_ppt_deck::summary)?;
    respond(&ctx, &summary)
}

/// Writes one open deck to a native project path.
#[function]
fn deck_save(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let path = text(&ctx, 1)?;
    let overwrite = ctx.args().get(2).map(|value| value.as_bool()).transpose()?.unwrap_or(false);
    let report = with_deck(handle, |deck| {
        open_ppt_deck::save(deck, std::path::Path::new(&path), overwrite)
            .map(|bytes| json!({ "path": path, "revision": deck.revision(), "slides": deck.slides().len(), "bytes": bytes }))
    })?
    .map_err(fail)?;
    respond(&ctx, &report)
}

/// Runs the rule checks of one open deck.
#[function]
fn deck_inspect(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let checks = match ctx.args().get(1) {
        Some(value) => {
            let raw = value.as_string()?;
            let names: Vec<String> =
                serde_json::from_str(&raw).map_err(|error| Error::new(format!("session/argument-invalid: {error}")))?;
            let mut parsed = Vec::with_capacity(names.len());
            for name in names {
                parsed.push(
                    InspectCheck::parse(&name)
                        .ok_or_else(|| Error::new(format!("session/argument-invalid: unknown check `{name}`")))?,
                );
            }
            Some(parsed)
        }
        None => None,
    };
    let report = with_deck(handle, |deck| open_ppt_deck::inspect(deck, checks))?;
    respond(&ctx, &report)
}

/// Returns the static playback frames of one open deck.
#[function]
fn deck_play(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let frames = with_deck(handle, open_ppt_deck::play)?.map_err(fail)?;
    respond(&ctx, &json!({ "frames": frames }))
}

/// Applies one atomic JSON edit script and returns the committed summary.
#[function]
fn deck_edit(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let raw = text(&ctx, 1)?;
    let request: Value =
        serde_json::from_str(&raw).map_err(|error| Error::new(format!("session/argument-invalid: {error}")))?;
    let report = with_deck_mut(handle, |deck| open_ppt_deck::edit::edit(deck, &request))?.map_err(fail)?;
    respond(&ctx, &report)
}

/// Returns one slide with its elements in authoring units.
#[function]
fn deck_slide(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let slide = text(&ctx, 1)?;
    let detail = with_deck(handle, |deck| open_ppt_deck::edit::slide_detail(deck, &slide))?.map_err(fail)?;
    respond(&ctx, &detail)
}

/// Lists embedded assets with sizes and hashes.
#[function]
fn deck_assets(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let report = with_deck(handle, open_ppt_deck::edit::assets)?;
    respond(&ctx, &report)
}

/// Returns one embedded asset with base64 bytes for preview surfaces.
#[function]
fn deck_asset(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let asset = text(&ctx, 1)?;
    let detail = with_deck(handle, |deck| open_ppt_deck::edit::asset_bytes(deck, &asset))?.map_err(fail)?;
    respond(&ctx, &detail)
}

/// Restores the previous committed document.
#[function]
fn deck_undo(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let report = with_deck_mut(handle, open_ppt_deck::edit::undo)?;
    respond(&ctx, &report)
}

/// Reapplies the last undone document.
#[function]
fn deck_redo(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let report = with_deck_mut(handle, open_ppt_deck::edit::redo)?;
    respond(&ctx, &report)
}

/// Closes one deck handle and releases its resources.
#[function]
fn deck_close(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    if !remove(handle) {
        return Err(Error::new("deck/not-found: the deck handle is not open"));
    }
    ctx.env().boolean(true)
}

/// Renders every slide of one open deck to a standalone SVG document.
///
/// The optional second argument is a JSON request; only `{"format":"svg"}` is
/// served and every other format is refused instead of being approximated.
#[function]
fn deck_render(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let request = optional_json(&ctx, 1)?;
    check_render_format(&request)?;
    let report = with_deck(handle, open_ppt_deck::render)?.map_err(fail)?;
    respond(&ctx, &report)
}

/// Writes one open deck as a PresentationML package.
///
/// The request is JSON and carries `format`, `target`, `overwrite` and an
/// optional `lossPolicy`.
#[function]
fn deck_export(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let request = optional_json(&ctx, 1)?;
    let report = with_deck(handle, |deck| open_ppt_deck::export(deck, &request))?.map_err(fail)?;
    respond(&ctx, &report)
}

/// Imports a PresentationML package and renders every visible slide.
///
/// The reply is `{ handle, presentation }`, where `presentation` carries the
/// canvas size, one entry per visible slide and the collected issues.
#[function]
fn pptx_open(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let playback = open_ppt_deck::pptx::open(Path::new(&path)).map_err(fail)?;
    let summary = playback.summary();
    let handle = insert_playback(playback);
    respond(&ctx, &json!({ "handle": handle, "presentation": summary }))
}

/// Returns one painted slide of an imported presentation.
#[function]
fn pptx_slide(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    let raw = argument(&ctx, 1)?.as_i64()?;
    let index =
        u32::try_from(raw).map_err(|_| Error::new("session/argument-invalid: the slide number must be a positive integer"))?;
    let slide = with_playback(handle, |playback| playback.slide(index)).and_then(|slide| slide.map_err(fail))?;
    respond(&ctx, &json!({ "slide": slide }))
}

/// Closes one playback handle and releases its rendered slides.
#[function]
fn pptx_close(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    if !remove_playback(handle) {
        return Err(Error::new("object/not-found: the presentation handle is not open"));
    }
    ctx.env().boolean(true)
}

// One WebView host per process, owned by the thread that opened it.
thread_local! {
    static GUI: RefCell<Option<GuiState>> = const { RefCell::new(None) };
}

const GUI_CLOSED: &str = "gui_closed";
const GUI_HOST_EXITED: &str = "gui_host_exited";
const GUI_NOT_OPEN: &str = "gui_not_open";

struct GuiState {
    host: WebViewHost,
    backend: &'static str,
    opened_at: Instant,
    /// Set when the page asks the host to close, so a dead host is
    /// distinguishable from one that never processed the request.
    close_requested: Arc<AtomicBool>,
}

struct DeckIpcHandler {
    close_requested: Arc<AtomicBool>,
}

impl IpcHandler for DeckIpcHandler {
    fn on_ipc(&mut self, message: &str, _sink: &IpcSink) -> IpcOutcome {
        let Ok(value) = serde_json::from_str::<Value>(message)
        else {
            return IpcOutcome::Ignored;
        };
        let ty = value.get("type").and_then(Value::as_str).unwrap_or("");
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        match ty {
            "close" => {
                self.close_requested.store(true, Ordering::SeqCst);
                IpcOutcome::CloseWindow
            }
            "ready" => reply(id, "readyResult", json!({ "version": env!("CARGO_PKG_VERSION"), "platform": platform_name() })),
            "capabilities" => reply(id, "capabilitiesResult", json!({ "capabilities": open_ppt_deck::capabilities() })),
            "deck.create" => {
                let request = value.get("request").cloned().unwrap_or(Value::Null);
                match call_create(&request) {
                    Ok(result) => reply(id, "createResult", result),
                    Err(message) => failure(id, "createError", message),
                }
            }
            "deck.open" => {
                let path = value.get("path").and_then(Value::as_str).unwrap_or("").trim().to_string();
                if path.is_empty() {
                    return failure(id, "openError", "an open request needs a project path".to_string());
                }
                match call_open(&path) {
                    Ok(result) => reply(id, "openResult", result),
                    Err(message) => failure(id, "openError", message),
                }
            }
            "deck.summary" => match call_summary(handle_of(&value)) {
                Ok(summary) => reply(id, "summaryResult", json!({ "deck": summary })),
                Err(message) => failure(id, "summaryError", message),
            },
            "deck.slide" => {
                let slide_id = value.get("slideId").and_then(Value::as_str).unwrap_or("");
                match call_slide(handle_of(&value), slide_id) {
                    Ok(slide) => reply(id, "slideResult", json!({ "slide": slide })),
                    Err(message) => failure(id, "slideError", message),
                }
            }
            "deck.assets" => {
                let handle = handle_of(&value);
                match deck_call(handle, open_ppt_deck::edit::assets) {
                    Ok(assets) => reply(id, "assetsResult", assets),
                    Err(message) => failure(id, "assetsError", message),
                }
            }
            "deck.asset" => {
                let asset_id = value.get("assetId").and_then(Value::as_str).unwrap_or("");
                let handle = handle_of(&value);
                match deck_service_call(handle, |deck| open_ppt_deck::edit::asset_bytes(deck, asset_id)) {
                    Ok(asset) => reply(id, "assetResult", asset),
                    Err(message) => failure(id, "assetError", message),
                }
            }
            "deck.save" => {
                let path = value.get("path").and_then(Value::as_str).unwrap_or("").trim().to_string();
                if path.is_empty() {
                    return failure(id, "saveError", "a save request needs a project path".to_string());
                }
                let overwrite = value.get("overwrite").and_then(Value::as_bool).unwrap_or(true);
                match call_save(handle_of(&value), &path, overwrite) {
                    Ok(report) => reply(id, "saveResult", json!({ "report": report })),
                    Err(message) => failure(id, "saveError", message),
                }
            }
            "deck.edit" => {
                let request = value.get("request").cloned().unwrap_or(Value::Null);
                match call_edit(handle_of(&value), &request) {
                    Ok(report) => reply(id, "editResult", json!({ "report": report })),
                    Err(message) => failure(id, "editError", message),
                }
            }
            "deck.undo" => {
                let handle = handle_of(&value);
                match deck_call_mut(handle, open_ppt_deck::edit::undo) {
                    Ok(report) => reply(id, "historyResult", json!({ "report": report })),
                    Err(message) => failure(id, "historyError", message),
                }
            }
            "deck.redo" => {
                let handle = handle_of(&value);
                match deck_call_mut(handle, open_ppt_deck::edit::redo) {
                    Ok(report) => reply(id, "historyResult", json!({ "report": report })),
                    Err(message) => failure(id, "historyError", message),
                }
            }
            "deck.render" => {
                let request = value.get("request").cloned().unwrap_or(Value::Null);
                match call_render(handle_of(&value), &request) {
                    Ok(report) => reply(id, "renderResult", json!({ "report": report })),
                    Err(message) => failure(id, "renderError", message),
                }
            }
            "deck.export" => {
                let request = value.get("request").cloned().unwrap_or(Value::Null);
                match call_export(handle_of(&value), &request) {
                    Ok(report) => reply(id, "exportResult", json!({ "report": report })),
                    Err(message) => failure(id, "exportError", message),
                }
            }
            "deck.close" => {
                let handle = handle_of(&value);
                if remove(handle) {
                    reply(id, "closedResult", json!({ "closed": true }))
                }
                else {
                    failure(id, "closeError", "the deck handle is not open".to_string())
                }
            }
            "pptx.open" => {
                let path = value.get("path").and_then(Value::as_str).unwrap_or("").trim().to_string();
                if path.is_empty() {
                    return failure(id, "pptxOpenError", "an open request needs a presentation path".to_string());
                }
                match call_pptx_open(&path) {
                    Ok(opened) => reply(id, "pptxOpenResult", opened),
                    Err(message) => failure(id, "pptxOpenError", message),
                }
            }
            "pptx.slide" => {
                let handle = handle_of(&value);
                let index = value.get("index").and_then(Value::as_u64).and_then(|raw| u32::try_from(raw).ok()).unwrap_or(0);
                match call_pptx_slide(handle, index) {
                    Ok(slide) => reply(id, "pptxSlideResult", json!({ "slide": slide })),
                    Err(message) => failure(id, "pptxSlideError", message),
                }
            }
            "pptx.close" => {
                let handle = handle_of(&value);
                if remove_playback(handle) {
                    reply(id, "pptxClosedResult", json!({ "closed": true }))
                }
                else {
                    failure(id, "pptxCloseError", "the presentation handle is not open".to_string())
                }
            }
            _ => IpcOutcome::Ignored,
        }
    }
}

fn reply(id: Value, kind: &str, payload: Value) -> IpcOutcome {
    let mut body = payload;
    body["type"] = Value::String(kind.to_string());
    body["id"] = id;
    IpcOutcome::ReplyToPage(body.to_string())
}

fn failure(id: Value, kind: &str, message: String) -> IpcOutcome {
    reply(id, kind, json!({ "message": message }))
}

fn handle_of(value: &Value) -> u64 {
    value.get("handle").and_then(Value::as_u64).unwrap_or(0)
}

fn service_failure(error: ServiceError) -> String {
    format!("{}: {}", error.code, error.message)
}

fn deck_call<T>(handle: u64, work: impl FnOnce(&Deck) -> T) -> std::result::Result<T, String> {
    with_deck(handle, work).map_err(|error| error.to_string())
}

fn deck_call_mut<T>(handle: u64, work: impl FnOnce(&mut Deck) -> T) -> std::result::Result<T, String> {
    with_deck_mut(handle, work).map_err(|error| error.to_string())
}

fn deck_service_call<T>(
    handle: u64,
    work: impl FnOnce(&Deck) -> std::result::Result<T, ServiceError>,
) -> std::result::Result<T, String> {
    match with_deck(handle, work) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(service_failure(error)),
        Err(error) => Err(error.to_string()),
    }
}

fn call_create(request: &Value) -> std::result::Result<Value, String> {
    let slides = request["slides"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let deck = open_ppt_deck::create(CreateRequest {
        title: request["title"].as_str().map(str::to_string),
        size: parse_size(request).map_err(|error| error.to_string())?,
        theme: request["theme"].as_str().map(str::to_string),
        slides,
    })
    .map_err(service_failure)?;
    let summary = open_ppt_deck::summary(&deck);
    let handle = insert(deck);
    Ok(json!({ "handle": handle, "deck": summary }))
}

fn call_open(path: &str) -> std::result::Result<Value, String> {
    let deck = open_ppt_deck::open(Path::new(path)).map_err(service_failure)?;
    let summary = open_ppt_deck::summary(&deck);
    let handle = insert(deck);
    Ok(json!({ "handle": handle, "deck": summary }))
}

fn call_summary(handle: u64) -> std::result::Result<Value, String> {
    deck_call(handle, open_ppt_deck::summary)
}

fn call_slide(handle: u64, slide_id: &str) -> std::result::Result<Value, String> {
    deck_service_call(handle, |deck| open_ppt_deck::edit::slide_detail(deck, slide_id))
}

fn call_save(handle: u64, path: &str, overwrite: bool) -> std::result::Result<Value, String> {
    match with_deck(handle, |deck| {
        open_ppt_deck::save(deck, Path::new(path), overwrite)
            .map(|bytes| json!({ "path": path, "revision": deck.revision(), "slides": deck.slides().len(), "bytes": bytes }))
    }) {
        Ok(Ok(report)) => Ok(report),
        Ok(Err(error)) => Err(service_failure(error)),
        Err(error) => Err(error.to_string()),
    }
}

fn call_edit(handle: u64, request: &Value) -> std::result::Result<Value, String> {
    match with_deck_mut(handle, |deck| open_ppt_deck::edit::edit(deck, request)) {
        Ok(Ok(report)) => Ok(report),
        Ok(Err(error)) => Err(service_failure(error)),
        Err(error) => Err(error.to_string()),
    }
}

fn call_render(handle: u64, request: &Value) -> std::result::Result<Value, String> {
    check_render_format(request).map_err(|error| error.to_string())?;
    deck_service_call(handle, open_ppt_deck::render)
}

fn call_export(handle: u64, request: &Value) -> std::result::Result<Value, String> {
    deck_service_call(handle, |deck| open_ppt_deck::export(deck, request))
}

fn call_pptx_open(path: &str) -> std::result::Result<Value, String> {
    let playback = open_ppt_deck::pptx::open(Path::new(path)).map_err(service_failure)?;
    let presentation = playback.summary();
    let handle = insert_playback(playback);
    Ok(json!({ "handle": handle, "presentation": presentation }))
}

fn call_pptx_slide(handle: u64, index: u32) -> std::result::Result<Value, String> {
    match with_playback(handle, |playback| playback.slide(index)) {
        Ok(Ok(slide)) => Ok(slide),
        Ok(Err(error)) => Err(service_failure(error)),
        Err(error) => Err(error.to_string()),
    }
}

fn platform_name() -> &'static str {
    if cfg!(windows) {
        "win32"
    }
    else if cfg!(target_os = "macos") {
        "darwin"
    }
    else {
        "linux"
    }
}

fn gui_status(
    state: &'static str,
    code: Option<&'static str>,
    backend: Option<&'static str>,
    uptime_ms: u64,
    message: Option<String>,
) -> Value {
    json!({
        "state": state,
        "code": code,
        "backend": backend,
        "platform": platform_name(),
        "uptimeMs": uptime_ms,
        "message": message,
    })
}

fn gui_open_result(ok: bool, status: Value, error: Option<&WebViewError>) -> Value {
    let (stage, hresult, attempted) = match error {
        Some(error) => {
            let attempted = match error {
                WebViewError::Host(host) => host.attempted.clone(),
                _ => Vec::new(),
            };
            (error.stage(), error.hresult(), attempted)
        }
        None => (None, None, Vec::new()),
    };
    json!({
        "ok": ok,
        "status": status,
        "stage": stage,
        "hresult": hresult,
        "attempted": attempted,
    })
}

/// Resolves the GUI content directory: explicit override, then the built tree.
fn resolve_gui_dir() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("OPEN_PPT_GUI") {
        let path = PathBuf::from(value);
        if path.is_dir() {
            return Some(path);
        }
        if path.is_file() {
            return path.parent().map(Path::to_path_buf);
        }
    }

    let manifest_relative = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("projects")
        .join("packages")
        .join("open-ppt")
        .join("dist")
        .join("gui");
    if manifest_relative.join("index.html").is_file() {
        return Some(manifest_relative);
    }

    if let Ok(cwd) = std::env::current_dir() {
        for candidate in [cwd.join("projects/packages/open-ppt/dist/gui"), cwd.join("dist/gui"), cwd.join("gui")] {
            if candidate.join("index.html").is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Opens the OpenPPT WebView window, or focuses the live one.
#[function(js_name = "openGui")]
fn open_gui(ctx: CallContext) -> Result<JsValue> {
    let mut title = "OpenPPT".to_string();
    let mut content_dir: Option<String> = None;
    let mut width = 1280i32;
    let mut height = 820i32;

    if let Some(arg) = ctx.args().first().copied()
        && !arg.is_undefined()?
    {
        if let Some(value) = optional_string_prop(arg, "title")? {
            title = value;
        }
        if let Some(value) = optional_string_prop(arg, "contentDir")? {
            content_dir = Some(value);
        }
        if let Ok(value) = arg.get_named("width")
            && let Ok(number) = value.as_i64()
        {
            width = number as i32;
        }
        if let Ok(value) = arg.get_named("height")
            && let Ok(number) = value.as_i64()
        {
            height = number as i32;
        }
    }

    let live = GUI.with(|slot| {
        let guard = slot.borrow();
        guard
            .as_ref()
            .filter(|state| state.host.is_alive())
            .map(|state| (state.backend, state.opened_at.elapsed().as_millis() as u64))
    });
    if let Some((backend, uptime_ms)) = live {
        GUI.with(|slot| {
            if let Some(state) = slot.borrow_mut().as_mut() {
                let _ = state.host.focus();
            }
        });
        let status = gui_status("open", None, Some(backend), uptime_ms, None);
        return respond(&ctx, &gui_open_result(true, status, None));
    }
    GUI.with(|slot| {
        *slot.borrow_mut() = None;
    });

    let dir = match content_dir {
        Some(dir) => PathBuf::from(dir),
        None => match resolve_gui_dir() {
            Some(dir) => dir,
            None => {
                let status = gui_status(
                    "failed",
                    Some("gui_content_missing"),
                    None,
                    0,
                    Some("the WebView content directory is missing; build the GUI first".to_string()),
                );
                return respond(&ctx, &gui_open_result(false, status, None));
            }
        },
    };

    let close_requested = Arc::new(AtomicBool::new(false));
    let mut options = OpenOptions::from_content_dir(dir.display().to_string(), title);
    options.width = width;
    options.height = height;
    options.accept_file_drop = true;

    match WebViewHost::open(options, Box::new(DeckIpcHandler { close_requested: Arc::clone(&close_requested) })) {
        Ok(host) => {
            let backend = host.backend_name();
            GUI.with(|slot| {
                *slot.borrow_mut() = Some(GuiState { host, backend, opened_at: Instant::now(), close_requested });
            });
            let status = gui_status("open", None, Some(backend), 0, None);
            respond(&ctx, &gui_open_result(true, status, None))
        }
        Err(error) => {
            let status = gui_status("failed", error.code(), None, 0, Some(error.to_string()));
            respond(&ctx, &gui_open_result(false, status, Some(&error)))
        }
    }
}

/// Pumps the host message loop once and reports the observed lifecycle state.
#[function(js_name = "pumpEvents")]
fn pump_events(ctx: CallContext) -> Result<JsValue> {
    let status = GUI.with(|slot| {
        let mut guard = slot.borrow_mut();
        let Some(state) = guard.as_mut()
        else {
            return gui_status("closed", Some(GUI_NOT_OPEN), None, 0, Some("no OpenPPT window is open".to_string()));
        };
        let backend = state.backend;
        let uptime_ms = state.opened_at.elapsed().as_millis() as u64;
        if state.host.pump() {
            return gui_status("open", None, Some(backend), uptime_ms, None);
        }
        let requested = state.close_requested.swap(false, Ordering::SeqCst);
        *guard = None;
        if requested {
            gui_status("closed", Some(GUI_CLOSED), Some(backend), uptime_ms, None)
        }
        else {
            gui_status(
                "closed",
                Some(GUI_HOST_EXITED),
                Some(backend),
                uptime_ms,
                Some(format!("the {backend} webview host stopped without a page close request")),
            )
        }
    });
    respond(&ctx, &status)
}

/// Closes the OpenPPT window; the result distinguishes "already closed".
#[function(js_name = "closeGui")]
fn close_gui(ctx: CallContext) -> Result<JsValue> {
    let result = GUI.with(|slot| {
        let mut guard = slot.borrow_mut();
        let Some(state) = guard.as_mut()
        else {
            return json!({
                "closed": false,
                "status": gui_status("not_open", Some(GUI_NOT_OPEN), None, 0, Some("no OpenPPT window is open".to_string())),
            });
        };
        let backend = state.backend;
        let uptime_ms = state.opened_at.elapsed().as_millis() as u64;
        match state.host.close() {
            Ok(()) => {
                *guard = None;
                json!({ "closed": true, "status": gui_status("closed", None, Some(backend), uptime_ms, None) })
            }
            Err(error) => json!({
                "closed": false,
                "status": gui_status("open", Some("gui_close_failed"), Some(backend), uptime_ms, Some(error.to_string())),
            }),
        }
    });
    respond(&ctx, &result)
}

/// Reports whether the OpenPPT window is alive.
#[function(js_name = "isGuiOpen")]
fn is_gui_open(ctx: CallContext) -> Result<JsValue> {
    let open = GUI.with(|slot| slot.borrow().as_ref().is_some_and(|state| state.host.is_alive()));
    ctx.env().boolean(open)
}

register_module![
    version,
    capabilities,
    deck_create,
    deck_open,
    deck_summary,
    deck_save,
    deck_inspect,
    deck_play,
    deck_edit,
    deck_slide,
    deck_assets,
    deck_asset,
    deck_undo,
    deck_redo,
    deck_render,
    deck_export,
    deck_close,
    pptx_open,
    pptx_slide,
    pptx_close,
    open_gui,
    pump_events,
    close_gui,
    is_gui_open
];
