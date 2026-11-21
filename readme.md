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
| Native project | supported | create, edit, undo/redo, save, static playback |
| PPTX import and playback | partial | `.pptx`-family packages open as editable decks: text, placeholders, pictures, preset shapes and tables are carried. Charts, SmartArt, OLE objects, media, groups and slide backgrounds are refused or reported with stable loss codes |
| PPTX / PPSX / POTX export | partial | text, preset shapes, pictures, tables and slide backgrounds are written; charts, formulas, media and groups are skipped and reported |
| SVG rendering | supported | one standalone SVG page per slide |
| PNG / PDF rendering | unsupported | needs a raster backend this build does not ship |
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
open-ppt create --title "Research review" --size 16:9 --out deck.project
open-ppt open deck.project
open-ppt import slides.pptx --out deck.project
open-ppt inspect deck.project
open-ppt render deck.project --format svg --out slides/
open-ppt export deck.project --format pptx --out output.pptx --overwrite
open-ppt play deck.pptx
```

```ts
import { OpenPPT } from '@wae/open-ppt';

const ppt = new OpenPPT();
const deck = await ppt.importPptx('slides.pptx');
console.log(deck.importLosses); // stable import.pptx.* codes, never silent drops
await deck.render({ format: 'svg', target: 'slides/' });
await deck.export({ format: 'pptx', target: 'output.pptx' });
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
