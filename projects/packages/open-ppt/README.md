# @wae/open-ppt

TypeScript authoring surface for OpenPPT: one editable deck model behind
playback, visual editing and agent authoring. The session entry point is
`new OpenPPT(options?)`.

## Status

Declared surface only. `new OpenPPT(options?)` creates a session synchronously,
and the package exports the frozen authoring types: deck/slide/element handles,
inspect/render/export/save reporting, playback, the stable error codes, and the
presentation format registry (`.pptx`/`.pptm`/`.ppsx`/`.ppsm`/`.potx`/`.potm`/
`.thmx`/`.ppam`/`.ppa`/`.ppt`/`.pps`/`.pot`/`.xml`/`.odp`/`.pdf`/`.xps`/video/
images).

Every capability still rejects with `capability/not-implemented`. Classification
is not a support claim: reads, writes and playback are reported by
`capabilities()` once the shared Rust capability tree lands, never inferred from
an extension or a method name.

## Boundaries

- No PPTX parsing or writing, layout, rendering, ZIP or XML code in this package.
- No second deck model: playback and editing consume the same project revision.
- Native access goes through the Node-API binding in `projects/crates/napi`.

## Scripts

```bash
pnpm --filter @wae/open-ppt run typecheck
pnpm --filter @wae/open-ppt run build
pnpm --filter @wae/open-ppt run test
```
