//! Native host entry for OpenPPT.
//!
//! Scaffold only. This binary reports that no deck authoring, playback or
//! export capability exists yet. The WAE host assembly, deck lifecycle and CLI
//! task tree land with the first implemented stage. Document semantics, layout,
//! rendering and PPTX exchange stay in the shared upstream capability tree and
//! must not be re-implemented here.

fn main() {
    eprintln!("open-ppt: scaffold only - no deck authoring, playback or export capability is implemented yet");
    std::process::exit(2);
}
