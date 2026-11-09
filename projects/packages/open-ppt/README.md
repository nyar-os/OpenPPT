# @wae/open-ppt

TypeScript authoring surface for OpenPPT: one editable deck model behind
playback, visual editing and agent authoring. The session entry point is
`new OpenPPT(options?)`.

## Status

Scaffold only. The package exports no authoring API yet. `create`, `open`, the
deck/slide/object model, playback, review, rendering and PPTX exchange are
implemented in later stages on top of the shared Rust capability tree.

## Boundaries

- No PPTX parsing or writing, layout, rendering, ZIP or XML code in this package.
- No second deck model: playback and editing consume the same project revision.
- Native access goes through the Node-API binding in `crates/napi`.

## Scripts

```bash
pnpm --filter @wae/open-ppt run typecheck
pnpm --filter @wae/open-ppt run build
```
