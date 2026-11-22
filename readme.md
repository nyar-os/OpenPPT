# OpenPPT

![.pptx](https://img.shields.io/badge/format-pptx-blue)
![Editable Deck](https://img.shields.io/badge/deck-editable-brightgreen)
![Template Fill](https://img.shields.io/badge/template-placeholder--fill-blue)

CLI/SDK for `.pptx` generation, editable decks, and template-based presentation authoring. Playback, visual editing, and agent authoring all operate on the same deck model — no format loss between entry points.

## 🧩 Architecture

```mermaid
graph LR
    A[.pptx file] --> B[OpenPPT]
    C[CLI · SDK · Agent] --> B
    B --> D[Native bridge]
    D --> E[Document processing engine]
    F[UI (VMZ)] --> B
```

## Capabilities

The engine reports what it can actually do; `open-ppt capabilities` is the
source of truth. This build wires:

| Area | Status | Notes |
| --- | --- | --- |
| Native project | supported | create, JSON edit scripts (slides, elements, element duplication, alignment and distribution, assets), undo/redo, save, static playback |
| PPTX import and playback | partial | `.pptx`-family packages open as editable decks: text, placeholders, pictures, preset shapes, tables and slide backgrounds are carried. Charts, SmartArt, OLE objects, media and groups are refused or reported with stable loss codes |
| PPTX / PPSX / POTX export | partial | text, preset shapes, pictures, tables and slide backgrounds are written; charts, formulas, media and groups are skipped and reported |
| SVG rendering | supported | one standalone SVG page per slide |
| PDF export | partial | a static projection paints slide backgrounds, text, tables, preset shapes and JPEG/PNG pictures; PNG transparency becomes a soft mask, while charts, formulas, media, groups and other picture encodings are skipped and reported |
| PNG rendering | unsupported | needs a raster backend this build does not ship |
| Timed builds, transitions, media playback | unsupported | the static model has no timeline |

Losses are returned as data (`losses` with a stable `code`), never as invisible
approximations. An export with `lossPolicy: 'error'` refuses to write when
anything would be dropped.

## What's NOT here

- **No full OOXML engine** — parsing, layout math, text shaping and playback live in the shared processing engine. This repo adds a small PresentationML package writer on top of the deck model.
- **No slide or deck algorithms** — master/layout inheritance, timeline math, overflow detection and contrast checks belong upstream.
- **No fixture corpus** — golden `.pptx` files, visual baselines, and heavyweight acceptance suites live downstream. This repo keeps minimal smoke-level tests only.
- **No second deck model** — playback, editing and agent authoring consume the same project revision. There is no "playback deck" vs. "authoring deck" split.
- **No algorithms in the skills package** — it ships skill documents only.

## 🚀 Usage

```bash
open-ppt capabilities
open-ppt create --title "Research review" --size 16:9 --theme default --out deck.project
open-ppt open deck.project
open-ppt import slides.pptx --out deck.project
open-ppt edit deck.project --ops '[{"op":"slide.add"}]' --out deck.project --overwrite
open-ppt edit deck.project --ops '[{"op":"element.align","slideId":"slide-1","elementIds":["element-1","element-2"],"edge":"left"}]'
open-ppt edit deck.project --ops '[{"op":"element.align","slideId":"slide-1","elementIds":["element-1","element-2"],"edge":"center","relativeTo":"selection"}]'
open-ppt edit deck.project --ops '[{"op":"element.distribute","slideId":"slide-1","elementIds":["element-1","element-2","element-3"],"axis":"horizontal"}]'
open-ppt edit deck.project --ops '[{"op":"element.duplicate","slideId":"slide-1","elementId":"element-1","offsetXPt":12,"offsetYPt":12}]'
open-ppt edit deck.project --script edits.json --expected-revision 3 --out deck.project --overwrite
open-ppt slide deck.project <slideId>
open-ppt assets deck.project
open-ppt undo deck.project --out deck.project --overwrite
open-ppt inspect deck.project
open-ppt render deck.project --format svg --out slides/
open-ppt contact-sheet deck.project --out contact-sheet.svg --columns 4
open-ppt export deck.project --format pptx --out output.pptx --overwrite
open-ppt play deck.pptx
```

`--script` accepts either an operations array or a full edit request
(`{ "operations": [...], "expectedRevision": 3 }`), so a script can pin the
revision it was written against. A failed edit writes no output file.

`element.align` pins one or more boxes to `left`, `center`, `right`, `top`,
`middle` or `bottom` of the slide (default) or of the selection's joint
bounding box (`"relativeTo": "selection"`). `element.distribute` needs at least
three boxes and equalises the gaps on `horizontal` or `vertical` while keeping
the first and last box in place. Both only change `x`/`y`, never sizes or
rotation, and run through the same draft, revision and undo path as every
other operation.

`element.duplicate` rebuilds one text, shape, table or image element as a new
element at the end of the slide, optionally shifted by `offsetXPt`/`offsetYPt`
(the SDK calls them `offsetX`/`offsetY`). `targetSlideId` (`toSlideId` in the
SDK) copies it onto another slide while leaving the source untouched. Charts,
formulas, media, cropped images and placeholder-bound text are refused with
`object/unsupported` instead of being copied partially.

```ts
import { OpenPPT } from '@wae/open-ppt';

const ppt = new OpenPPT();

const deck = await ppt.create({ title: 'Research review', size: '16:9', slides: ['Intro'] });
console.log(deck.summary()); // id, revision, size, layouts, slides

await deck.edit([{ op: 'slide.add', layout: 'title-body' }], { expectedRevision: deck.revision });
await deck.play(); // one static frame per build step
await deck.slide(deck.summary().slides[0].id);
await deck.export({ format: 'pptx', target: 'output.pptx' });

const imported = await ppt.importPptx('slides.pptx');
console.log(imported.importLosses); // stable import.pptx.* codes, never silent drops
await imported.render({ format: 'svg', target: 'slides/' });
await imported.contactSheet({ target: 'contact-sheet.svg', columns: 4 }); // one SVG grid of thumbnails

const reader = await ppt.readPptx('slides.pptx'); // playback without an editable deck
console.log(reader.slide(1).svg);
await ppt.dispose();
```

## 🧱 Repo layout

```
OpenPPT/
├── projects/
│   ├── crates/
│   │   ├── deck/                  # Deck model, editor, SVG renderer, PPTX writer
│   │   └── napi/                  # Node-API binding (thin value mapping)
│   ├── packages/
│   │   ├── open-ppt/              # CLI + SDK entry point
│   │   ├── open-ppt-skills/       # Agent skill documents
│   │   ├── open-ppt-ui/           # VMZ desktop & web UI
│   │   ├── open-ppt-win32-x64/    # Windows x64 native addon
│   │   ├── open-ppt-darwin-x64/   # macOS x64 native addon
│   │   ├── open-ppt-darwin-arm64/ # macOS ARM64 native addon
│   │   ├── open-ppt-linux-x64/    # Linux x64 native addon
│   │   └── open-ppt-linux-arm64/  # Linux ARM64 native addon
│   └── sites/                     # Homepage
├── package.json
└── pnpm-workspace.yaml
```
