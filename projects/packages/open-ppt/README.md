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
  PresentationML package, including tables, picture assets, slide backgrounds
  and speaker notes.
- `export({ format: 'pdf' })` writes a static PDF 1.7 projection with
  backgrounds, shapes, text, tables and JPEG pictures.
- `importPptx(path)` carries text, placeholders, pictures, preset shapes,
  tables, slide backgrounds and speaker notes. Refused constructs are listed on
  `deck.importLosses` with stable `import.pptx.*` codes instead of being faked.
- `capabilities()` reports the real matrix. Anything not wired is
  `unsupported`, or `partial` with its loss codes.

PPTX import and export remain `partial`: charts, SmartArt, OLE objects, media
and groups are skipped and reported, and non-solid backgrounds are flattened to
an explicit no-fill with a loss code. PDF export is `partial`: only JPEG
pictures are embedded, characters outside Latin-1 become `?`, and charts,
formulas, media and groups are reported instead of painted. PNG/PDF rendering,
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
