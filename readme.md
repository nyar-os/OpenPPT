# OpenPPT

![Editable decks](https://img.shields.io/badge/decks-editable-brightgreen)
![PPTX and PDF](https://img.shields.io/badge/export-pptx%20%7C%20pdf-blue)
![Agent ready](https://img.shields.io/badge/agent-ready-purple)

[Package](projects/packages/open-ppt/README.md) · [Agent guide](documentation/agents.md) · [License](License.md)

## 💡 What is OpenPPT?

OpenPPT is a code-first presentation toolkit: you author a deck programmatically, keep it editable, and export it as PPTX, PPSX, POTX or PDF.

A deck is a live model, not a generated file. You create slides, add text, images, shapes, tables, charts, formulas and media, place and stack elements, duplicate content across slides, write speaker notes, and save the project at a specific revision. From there you can render slides, build a whole-deck contact sheet, inspect text and geometry, search the deck, produce an outline, play the static show, undo or redo any committed edit, and export with a report of everything a target format could not keep.

It also reads decks that already exist. Importing a `.pptx`-family package gives you an editable deck plus a loss report, and a lightweight reader can play a package without building an authoring project.

OpenPPT is not a GUI slide editor. There is no drag-and-drop canvas in the core workflow; you describe the deck in code or natural language and review the rendered result. It is also not a pixel-perfect clone of PowerPoint: constructs the engine cannot carry are reported with stable codes instead of being silently approximated.

## 🚀 Getting started

Node.js 18 or newer is required. The CLI and SDK are published as `@wae/open-ppt`.

### 1. Agent / prompt (recommended)

Install the OpenPPT skill for your agent, then describe the deck you want. The skill document routes the work through capability checks, editing, rendering and delivery, and refuses routes the installed tool cannot actually execute.

Example prompt:

```text
Use the open-ppt skill to build a 6-slide "Quarterly review" deck from my notes.
Add one summary table and one column chart, keep the theme neutral, write speaker
notes for each slide, then export review.pptx and show me the loss report.
```

With that prompt, an agent typically runs:

```bash
open-ppt capabilities
open-ppt create --title "Quarterly review" --size 16:9 --slide Summary --slide "Key metrics" --out review.project
open-ppt edit review.project --ops '[{"op":"text.add","slideId":"slide-1","text":"Revenue is up 18% year over year"}]' --out review.project --overwrite
open-ppt export review.project --format pptx --out review.pptx --overwrite
```

The skill source lives in [skills/open-ppt/SKILL.md](projects/packages/open-ppt-skills/skills/open-ppt/SKILL.md). It ships with the `@wae/open-ppt-skills` package and never claims a command or flag the CLI does not expose.

### 2. Manual install

```bash
npm install -g @wae/open-ppt

open-ppt capabilities
open-ppt create --title "Product launch" --size 16:9 --out launch.project
open-ppt render launch.project --format svg --out slides/
open-ppt export launch.project --format pdf --out launch.pdf --overwrite
```

`open-ppt capabilities` prints what the installed engine supports. Read it before relying on a format: import, editing, export and playback are disclosed separately, and a `partial` entry always names the loss codes it can emit.

## ✨ Highlights

- **One editable deck model.** Playback, editing and agent authoring read and write the same project, with stable IDs and a monotonic revision on every object.
- **Programmatic authoring.** Slides, text, images, shapes, tables, charts, formulas, media, assets, titles, notes, hidden slides, slide backgrounds, slide layouts and deck-wide find and replace.
- **Real layout operations.** Align to the slide or to the selection, distribute three or more boxes, change Z-order, lock elements, transform them, or duplicate an element onto another slide.
- **Atomic edits.** Every edit script is one transaction. Pin `expectedRevision`, and a failed edit writes no output file and leaves no half-changed deck.
- **Undo and redo.** Any committed edit can be reverted and reapplied.
- **Review without opening PowerPoint.** SVG page rendering, a contact sheet of the whole deck, search across every slide, outlines with talking points, and geometry checks.
- **Honest export.** PPTX, PPSX, POTX and PDF export report dropped constructs with stable codes; `lossPolicy: 'error'` refuses to write instead of guessing.
- **Import and inspect existing decks.** Bring in a `.pptx`-family package as an editable deck with an import loss report, or play it through a lightweight reader.
- **Static playback.** Walk the show frame by frame. Timed builds, transitions and media playback are outside the static model.

## 📊 How OpenPPT compares

| | OpenPPT | GUI slide editor | One-shot PPTX generator |
| --- | --- | --- | --- |
| Primary workflow | Code or agent prompt, then render and review | Mouse and menus | Script that writes a file once |
| Deck after generation | Editable project with revisions, undo and redo | Editable by hand | Usually a dead artifact |
| Layout operations | Align, distribute, Z-order, lock, duplicate across slides | Manual dragging | Rarely exposed |
| Review | SVG render, contact sheet, search, outline, checks | On-screen only | Read the exported file |
| Loss reporting | Stable codes per dropped construct | Not applicable | Often silent |

## 🧪 Examples

| Goal | Start here |
| --- | --- |
| Build a deck from a brief with an agent | [Agent guide](documentation/agents.md) |
| Create, edit and export from TypeScript | [Package README](projects/packages/open-ppt/README.md) |
| Review and revise an existing deck | [skills/open-ppt/SKILL.md](projects/packages/open-ppt-skills/skills/open-ppt/SKILL.md) |
| Understand the CLI and edit-operation JSON | [Agent guide](documentation/agents.md) |

Sample deck names in this repository are deliberately neutral: `Quarterly review`, `Product launch`, `Research findings`, `Training session`.

## 📜 License

Apache-2.0. See [License.md](License.md).
