//! Thin Node-API surface for OpenPPT.
//!
//! Scaffold only. `version` reports the binding version so `@wae/open-ppt` can
//! tell an installed native binary apart from an absent one. Every authoring
//! entry point is declared and returns the stable `capability/not-implemented`
//! code instead of a fabricated deck, report or capability table.
//!
//! Deck model, layout, rendering and PPTX exchange stay in the shared upstream
//! capability tree; this layer only maps values.

#![deny(missing_docs)]

use wae::{CallContext, Error, JsValue, Result, function, register_module};

/// Stable code returned by every declared-but-unwired authoring entry point.
///
/// The TypeScript surface turns it into `NotImplementedError`. Native code never
/// returns an empty deck or an all-`unsupported` capability table, because that
/// would be indistinguishable from a working engine that reports limitations.
const NOT_IMPLEMENTED: &str = "capability/not-implemented";

/// Library version of the native binding.
#[function]
fn version(ctx: CallContext) -> Result<JsValue> {
    ctx.env().string(env!("CARGO_PKG_VERSION"))
}

/// Reports formats, checks, renderers and playback support of the engine.
#[function]
fn capabilities(_ctx: CallContext) -> Result<JsValue> {
    Err(Error::new(NOT_IMPLEMENTED))
}

/// Creates an empty deck.
#[function]
fn create(_ctx: CallContext) -> Result<JsValue> {
    Err(Error::new(NOT_IMPLEMENTED))
}

/// Opens a native project, `.pptx`-family file or in-memory package.
#[function]
fn open(_ctx: CallContext) -> Result<JsValue> {
    Err(Error::new(NOT_IMPLEMENTED))
}

/// Inspects an input without keeping a deck handle.
#[function]
fn inspect(_ctx: CallContext) -> Result<JsValue> {
    Err(Error::new(NOT_IMPLEMENTED))
}

register_module![version, capabilities, create, open, inspect];
