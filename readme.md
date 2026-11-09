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

## What's NOT here

- **No `.pptx` parser or writer implementation** — OOXML parse, slide layout, theme model, codecs and export live in the shared processing engine.
- **No slide or deck algorithms** — master/layout inheritance, timeline math, overflow detection and contrast checks belong upstream.
- **No fixture corpus** — golden `.pptx` files, visual baselines, and heavyweight acceptance suites live downstream. This repo keeps minimal smoke-level tests only.
- **No second deck model** — playback, editing and agent authoring consume the same project revision. There is no "playback deck" vs. "authoring deck" split.
- **No algorithms in the skills package** — it ships skill documents only.

## 🚀 Usage (placeholder)

Not implemented yet — the scaffold is ready. Commands follow the deck-centric model:

```bash
open-ppt create --title "Research review" --size 16:9
open-ppt open deck.pptx
open-ppt inspect deck.pptx
open-ppt render deck.pptx --format png --out-dir slides/
open-ppt export deck.pptx --format pptx -o output.pptx
```

```ts
import { OpenPPT } from '@wae/open-ppt';

const ppt = new OpenPPT();
const deck = await ppt.create({ title: 'Research review', size: '16:9', theme: 'editorial-light' });
await deck.slides.add({ layout: 'title-and-chart' });
await deck.export({ format: 'pptx', target: 'output.pptx' });
await ppt.dispose();
```

## 🧱 Repo layout

```
OpenPPT/
├── projects/
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

This is a scaffold repo. The processing engine wiring, CLI binary, and runtime entry points are added incrementally as the shared engine lands.
