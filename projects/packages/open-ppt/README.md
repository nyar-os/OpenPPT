# @wae/open-ppt

TypeScript authoring surface for OpenPPT: one editable deck model behind
playback, visual editing and agent authoring. The session entry point is
`new OpenPPT(options?)`.

## Status

The session entry point is `new OpenPPT(options?)`. This build wires one
editable deck model to the following operations:

- `create`, `open` and `importPptx` return editable decks with `save`, `edit`,
  `undo`/`redo` and static playback.
- `render({ format: 'svg' })` returns one standalone SVG page per slide.
- `export({ format: 'pptx' | 'ppsx' | 'potx' })` writes an editable
  PresentationML package, including tables and picture assets.
- `importPptx(path)` carries text, placeholders, pictures, preset shapes and
  tables. Refused constructs are listed on `deck.importLosses` with stable
  `import.pptx.*` codes instead of being faked.
- `capabilities()` reports the real matrix. Anything not wired is
  `unsupported`, or `partial` with its loss codes.

PPTX import and export remain `partial`: charts, SmartArt, OLE objects, media,
groups and slide backgrounds are skipped and reported. PNG/PDF rendering,
timed builds, transitions and media playback are not implemented.

## Boundaries

- PPTX parsing, layout math, text shaping and playback stay in the shared
  engine. This package only binds values and assembles the CLI.
- No second deck model: playback and editing consume the same project revision.
- Native access goes through the Node-API binding in `projects/crates/napi`.

## Scripts

```bash
pnpm --filter @wae/open-ppt run typecheck
pnpm --filter @wae/open-ppt run build
pnpm --filter @wae/open-ppt run test
```
