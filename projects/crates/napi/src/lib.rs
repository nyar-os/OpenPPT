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
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use open_ppt_deck::{CreateRequest, EditableDeck as Deck, InspectCheck, ServiceError, SizeRequest};
use serde_json::{Value, json};
use wae::{CallContext, Error, JsValue, Result, function, register_module};

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

fn remove(handle: u64) -> bool {
    lock().decks.remove(&handle).is_some()
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

/// Closes one deck handle and releases its resources.
#[function]
fn deck_close(ctx: CallContext) -> Result<JsValue> {
    let handle = handle(&ctx, 0)?;
    if !remove(handle) {
        return Err(Error::new("deck/not-found: the deck handle is not open"));
    }
    ctx.env().boolean(true)
}

register_module![version, capabilities, deck_create, deck_open, deck_summary, deck_save, deck_inspect, deck_play, deck_close];
