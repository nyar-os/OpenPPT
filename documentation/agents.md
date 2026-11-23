# Agent guide: OpenPPT

How an agent discovers capabilities, drives the `open-ppt` CLI and composes the `@wae/open-ppt` SDK. The CLI is the reliable surface for short, one-shot tasks such as `create`, `open`, `play`, `render` and `export`; complex editing is a TypeScript workflow built on `new OpenPPT()`.

- **GUI** (`open-ppt open <file>`, `open-ppt play <file>`): humans review layout, drag elements and run the show.
- **SDK**: anything data-driven or repeated — slide and element edits, notes, charts, inspection loops, previews, save and export.
- **Not delivered yet**: theme/layout enumeration and application, playback object creation, and autofit text. The engine reports these as `object/unsupported` with a feature name.

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
| `render <file> --out <dir>` | Render every slide to standalone SVG | `--format svg`, `--pages <list>` (one-based slide numbers) |
| `contact-sheet <file> --out <svg>` | Render slides into one SVG sheet | `--columns <n>` (default 3), `--pages <list>` (one-based slide numbers) |
| `export <file> --format <fmt> --out <file>` | Write PPTX, PPSX, POTX or PDF | `--overwrite` |
| `edit <file>` | Compatibility only: apply one atomic JSON edit script. New work should call `Deck.edit()` from a TypeScript script instead | `--ops <json>`, `--script <file>`, `--out <file>`, `--overwrite`, `--expected-revision <n>` |
| `slide <file> <slideId>` | Print one slide and its elements | — |
| `search <file> <query>` | Search visible text, or speaker notes with `--notes`, across slides | `--case-sensitive`, `--regex`, `--notes`, `--whole-word`, `--slide <id>` (repeatable, comma lists accepted; unknown ids ignored), `--kind <kind>` (repeatable, comma lists accepted; unknown kinds ignored), `--max-per-slide <n>`, `--limit <n>` (default 100) |
| `outline <file>` | Print the slide outline without rendering | `--points` |
| `assets <file>` | List embedded assets | — |
| `undo <file>` / `redo <file>` | Move one committed revision backward or forward | `--out <file>`, `--overwrite` |
| `gui <file>` | Open the editor window on the deck | — |

Project files are native deck files (for example `review.oppt`). Export formats currently are `pptx`, `ppsx`, `potx` and `pdf`.

Every `search` hit carries `text` (the whole title, element, table or notes unit), `start` and `length` (UTF-16 offsets inside `text`), and `context`: an excerpt with up to 40 characters on each side of the match, with `…` marking a clipped edge. `context` never splits a surrogate pair, so it is safe to print without re-slicing the full `text`.

The same matching options drive deck-wide rewrites. `{ "op": "deck.replaceText", "find": "revenue", "replace": "income", "wholeWord": true }` replaces every match inside the deck's text elements and table cells in one atomic edit and reports the number of replaced matches in `results[0]`; `caseSensitive` and `slideIds` behave exactly like their search counterparts, and a match spanning two text runs is left untouched.

## Recommended workflows

A user-facing session starts with one command:

```bash
open-ppt create review.oppt --title "Q4 review" --slide Highlights --slide Revenue --overwrite
open-ppt open review.oppt
```

Anything that positions elements, reacts to inspection findings or fills slides from data belongs in a script. The script edits by slide and element id, previews the current revision, fixes only the reported objects and then saves or exports. `edit --ops` and `edit --script` remain for compatibility with existing callers, but they are no longer the recommended agent path.

```ts
import { OpenPPT } from '@wae/open-ppt';

const ppt = new OpenPPT();
const deck = await ppt.create({ title: 'Q4 review', slides: ['Highlights', 'Revenue'] });
const [firstId, secondId] = deck.summary().slides.map((slide) => slide.id);
const first = await deck.slides.get(firstId);
const second = await deck.slides.get(secondId);

await first.title.set('Q4 highlights');
await first.notes.set('Lead with the 18% revenue lift.');
await first.elements.addText({
    text: 'Revenue is up 18%',
    transform: { xPt: 72, yPt: 140, widthPt: 480, heightPt: 90 },
});
await second.elements.addText({ text: 'Plan: expand to two regions in Q1' });

// Inspect the current revision, then fix only the object that was reported.
const inspection = await deck.inspect({ checks: ['overflow'] });
for (const issue of inspection.issues) {
    if (issue.elementId !== undefined) {
        await first.elements.update(issue.elementId, { text: 'Revenue is up 18%' });
    }
}

// Preview, export an exchange format, save the native project and reopen it.
await deck.render({ format: 'svg', target: 'preview' });
await deck.contactSheet({ target: 'review.svg', pages: [0, 2] });
await deck.export({ format: 'pptx', target: 'q4.pptx', overwrite: true });
await deck.save({ target: 'q4.oppt', overwrite: true });
await deck.close();

const reopened = await ppt.open({ path: 'q4.oppt' });
console.log(reopened.summary().revision, reopened.summary().slides.length);
await reopened.close();
```

`contactSheet({ pages })` accepts zero-based slide indexes or `'all'`; the sheet then holds only those slides while `slideIds`, `tiles` and the sheet size follow the filtered set. Omit `pages` for the whole deck. The CLI spells the same filter `--pages 1,3`, using one-based slide numbers for both `render` and `contact-sheet`.

`body` and the second slide id are real values returned by the engine; never invent ids. Report `export().losses` instead of claiming a lossless conversion.

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
| Deck | `deck.replaceText` |
| Slides | `slide.add`, `slide.duplicate`, `slide.remove`, `slide.move`, `slide.reorder`, `slide.setTitle`, `slide.setNotes`, `slide.setHidden`, `slide.setBackground`, `slide.setLayout` |
| Elements | `text.add`, `image.add`, `shape.add`, `table.add`, `chart.add`, `formula.add`, `media.add`, `element.remove`, `element.setText`, `element.setTextStyle`, `element.updateTransform`, `element.updateAlt`, `element.setChartData`, `element.setFill`, `element.setLine`, `table.setCell`, `table.setColumnWidths`, `table.setRowHeights` |
| Arrange | `element.align`, `element.distribute`, `element.duplicate`, `element.reorder`, `element.bringToFront`, `element.sendToBack`, `element.bringForward`, `element.sendBackward`, `element.lock` |
| Assets | `asset.add`, `asset.remove` |

`asset.remove` refuses an asset that image or media elements still reference with `object/unsupported`, and an unknown id is `object/not-found`. `slide.setLayout` applies a known layout id or clears it with `null`; an unknown slide or layout is `object/not-found`. Layout ids come from `summary().layouts`; the dedicated `layouts` and `themes` APIs are still not delivered. `element.setFill` repaints a shape's solid fill, or clears it with `null`; it targets shapes only, so any other element kind is `object/type-mismatch`. `element.setLine` likewise sets a shape's stroke from `{ color?, widthPt? }` (at least one field) or clears it with `null`. `table.setCell` rewrites one table cell by zero-based `row` and `column`, accepting `text` and/or `fill`; out-of-range coordinates or neither field are `session/argument-invalid`, and cell edits to one table compose inside a batch. `table.setColumnWidths` and `table.setRowHeights` resize a table from `columnWidthsPt` and `rowHeightsPt`; each list must have exactly one positive entry per column or row.

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
