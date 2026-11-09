---
name: open-ppt
description: >-
  Create, revise, review and deliver editable presentation decks with OpenPPT.
  Use when a task involves slide decks, PPTX exchange, template fill or deck
  review. The SDK and CLI are not implemented yet, so this skill records the
  routing and evidence rules only and must not invent OpenPPT calls.
---

# open-ppt

## Status

Scaffold only. `@wae/open-ppt` exposes no authoring API yet, and there is no
working CLI, renderer or PPTX exchange. Do not present any workflow below as
executable, and do not fill the gap with local XML, ZIP, typesetting or drawing
code.

## Preflight

Once the SDK/CLI ships, every task starts here:

1. Query the capabilities the installed SDK/CLI reports and confirm the format,
   object, playback and backend entries the task needs. A method name alone is
   not a capability.
2. Confirm the locale and the assets the task needs (fonts, images, data, brand
   template) are readable before generating anything.
3. If the preflight reports a missing capability, stop and report it. Never
   degrade silently or swap in an unrelated tool.

## Routes

| Route | Intent | Minimum evidence |
|-------|--------|------------------|
| create | Build a deck from a brief, materials or data | per-slide message plan, rendered contact sheet, per-page review |
| edit | Change an existing deck without losing style or identity | revision-scoped patch, re-render of affected pages, editability check |
| template | Fill a branded template through its placeholders | placeholder coverage report, overflow and contrast diagnostics |
| review | Audit narrative, layout, contrast, overflow and assets | diagnostics plus actual visual review, not a single score |
| deliver | Export editable PPTX plus needed previews and a report | independent reopen, loss report, remaining limitations |

## Hard rules

- Never export a deck whose pages are flattened images to dodge editability
  problems.
- Never claim a check ran when it did not, and never fabricate previews.
- PowerPoint or LibreOffice may serve only as an optional independent
  verification source. The workflow must not require either.
- Prefer native charts, tables, formulas and shapes over pasted pictures.
- Aesthetic and narrative quality still require looking at the rendered pages.

## References

None yet. Design rules, the support matrix and executable examples ship with
the first implemented release.
