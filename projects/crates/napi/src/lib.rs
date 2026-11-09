//! Thin Node-API surface for OpenPPT.
//!
//! Scaffold only. The single export is the binding version, which lets
//! `@wae/open-ppt` tell an installed native binary apart from an absent one.
//! Deck, slide, resource, playback and export APIs are not implemented and must
//! not be faked here: the shared upstream capability tree owns every domain
//! rule, and this layer only maps values.

#![deny(missing_docs)]

use wae::{CallContext, JsValue, Result, function, register_module};

/// Library version of the native binding.
#[function]
fn version(ctx: CallContext) -> Result<JsValue> {
    ctx.env().string(env!("CARGO_PKG_VERSION"))
}

register_module![version];
