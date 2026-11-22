# @wae/open-ppt-skills

Agent skill documents for OpenPPT. The package ships Markdown only: no runtime library, XML/ZIP code, typesetting or drawing algorithms.

## Usage

Install it next to `@wae/open-ppt`, then ask your agent to use the `open-ppt` skill:

```text
Use the open-ppt skill to import review.pptx, mark every slide title in the
speaker notes, and export a PDF for review.
```

The skill drives the `open-ppt` CLI and starts with `open-ppt capabilities --json`.

## Related

- [Product overview](../../../readme.md)
- [`@wae/open-ppt`](../open-ppt/README.md)
- [Agent guide](../../../documentation/agents.md)
