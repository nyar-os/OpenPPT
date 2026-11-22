# Agent guide: OpenPPT

How an agent discovers capabilities, drives the `open-ppt` CLI and speaks the edit-operation JSON shape. The CLI is the stable automation surface; the SDK exposes the same operations through `@wae/open-ppt`.

## Discover capabilities first

```bash
open-ppt capabilities
open-ppt capabilities --json
```

`capabilities` reports the installed engine, importable presentation formats, supported checks, renderers and export targets. Never infer support from a file extension or a method name: a `.pptx` path can still be rejected when the installed engine cannot import it.

Binary: `open-ppt` (`projects/packages/open-ppt/dist/cli.js`). Global flags: `--json` writes one machine envelope to stdout while help and diagnostics stay on stderr; `--no-interactive` is a hidden framework global; `--locale <id>` is registered by the locales loader.

## Command map

| Command | Purpose | Options |
|---|---|---|
| `capabilities` | Report what this build supports | `--json` |
| `create <out>` | Create a native deck project file | `--title`, `--size 16:9\|4:3\|16:10`, `--theme`, `--slide <title>...`, `--overwrite` |
| `open <file>` | Print the committed revision and slides | — |
| `import <pptx> --out <file>` | Import a PresentationML package as an editable project | `--overwrite` |
| `inspect <file>` | Run geometry and rule checks | `--checks overflow,contrast,alignment,assets,fonts,out-of-bounds` |
| `play <file>` | Walk the static show, or play a PPTX package | `--out <dir>` |
| `render <file> --out <dir>` | Render every slide to standalone SVG | `--format svg` |
| `contact-sheet <file> --out <svg>` | Render all slides into one SVG sheet | `--columns <n>` (default 3) |
| `export <file> --format <fmt> --out <file>` | Write PPTX, PPSX, POTX or PDF | `--overwrite` |
| `edit <file>` | Apply one atomic JSON edit script | `--ops <json>`, `--script <file>`, `--out <file>`, `--overwrite`, `--expected-revision <n>` |
| `slide <file> <slideId>` | Print one slide and its elements | — |
| `search <file> <query>` | Search visible text across slides | `--case-sensitive`, `--limit <n>` (default 100) |
| `outline <file>` | Print the slide outline without rendering | `--points` |
| `assets <file>` | List embedded assets | — |
| `undo <file>` / `redo <file>` | Move one committed revision backward or forward | `--out <file>`, `--overwrite` |
| `gui <file>` | Open the editor window on the deck | — |

Project files are native deck files (for example `review.oppt`). Export formats currently are `pptx`, `ppsx`, `potx` and `pdf`.

## Edit script shape

`edit` accepts either a bare operations array or a full request:

```json
{
  "expectedRevision": 4,
  "operations": [
    { "op": "slide.setTitle", "slideId": "slide-1", "text": "Account review" },
    { "op": "text.add", "slideId": "slide-1", "text": "Revenue is up 18%", "transform": { "xPt": 72, "yPt": 120, "widthPt": 480, "heightPt": 80 } }
  ]
}
```

Common operations:

| Family | Operations |
|---|---|
| Slides | `slide.add`, `slide.duplicate`, `slide.remove`, `slide.move`, `slide.reorder`, `slide.setTitle`, `slide.setNotes`, `slide.setHidden`, `slide.setBackground` |
| Elements | `text.add`, `image.add`, `shape.add`, `table.add`, `chart.add`, `formula.add`, `media.add`, `element.remove`, `element.setText`, `element.updateTransform`, `element.updateAlt`, `element.setChartData` |
| Arrange | `element.align`, `element.distribute`, `element.duplicate`, `element.reorder`, `element.bringToFront`, `element.sendToBack`, `element.bringForward`, `element.sendBackward`, `element.lock` |

A batch is atomic: it either advances the deck revision once or leaves the previous revision untouched. Pass `--expected-revision` to reject a stale writer with `deck/revision-conflict`.

## Failures

Every failure is an `OpenPPTError` with a stable `code`; never parse the message text.

| Code | Meaning |
|---|---|
| `session/argument-invalid` | Malformed option or payload |
| `deck/revision-conflict` | `expectedRevision` does not match the committed revision |
| `deck/not-found` | Project path or deck handle is unknown |
| `object/not-found` | Slide or element id is unknown |
| `object/type-mismatch` | Operation does not apply to that object type |
| `object/unsupported` | Operation exists but the installed engine cannot perform it |
| `format/unsupported` | Import or export format is unavailable |
| `format/invalid-package` | Input is not a valid PresentationML package |
| `format/macro-refused` | Macro-enabled input was refused by policy |
| `export/loss-exceeded` | Loss policy rejected the export |
| `render/backend-unavailable` | No renderer is available |
| `asset/missing` | Referenced asset is not embedded |
| `capability/not-implemented` | The installed build does not implement this entry point |

## Guardrails

- Call `capabilities()` before promising a format, check, renderer or export target.
- Store the returned `slideId` and `elementId` values; do not invent them.
- Use one atomic `edit` batch per logical change and carry `expectedRevision` when another writer may touch the file.
- Report export `losses` instead of claiming a lossless conversion. Use `lossPolicy: 'error'` when the task requires fidelity.
- Never claim a feature the installed build does not report through `capabilities()`.
