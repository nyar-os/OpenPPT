# @wae/open-ppt

The SDK and CLI behind OpenPPT decks: one session type, revisioned edit transactions, and format reports returned as plain values instead of thrown surprises.

## API surface

- `new OpenPPT(options?)` creates a session. The constructor opens no window, file or socket.
- `ppt.capabilities()` reports what the installed engine can import, edit, export and play.
- `ppt.create(options)`, `ppt.open(input)` and `ppt.importPptx(path)` return an editable `Deck`.
- `ppt.readPptx(path)` plays a `.pptx`-family package without building an authoring project.
- `ppt.inspect(input, options)` runs checks; `ppt.dispose()` releases every handle.
- `Deck` exposes `save`, `edit`, `transaction`, `undo`, `redo`, `search`, `outline`, `slide`, `play`, `inspect`, `render`, `contactSheet` and `export`.
- `deck.slides` adds, moves, duplicates, reorders and removes slides; `deck.assets` lists and embeds assets; `slide.elements` adds and updates text, images, shapes, tables, charts, formulas and media.
- Every failure is an `OpenPPTError` with a stable `code` such as `object/not-found`, `session/argument-invalid` or `object/unsupported`.

## Example

```ts
import { OpenPPT } from '@wae/open-ppt';

const ppt = new OpenPPT();
const deck = await ppt.create({ title: 'Quarterly review', size: '16:9', slides: ['Summary'] });
const [first] = await deck.slides.list();
const slide = await deck.slides.get(first.id);
await slide.elements.addText({ text: 'Revenue is up 18% year over year', transform: { xPt: 72, yPt: 120, widthPt: 480, heightPt: 80 } });
const { losses } = await deck.export({ format: 'pptx', target: 'review.pptx' });
console.log(losses);
await ppt.dispose();
```

`losses` lists every construct the export could not keep. Pass `lossPolicy: 'error'` to refuse writing instead.

## Related packages

- [OpenPPT project README](../../../readme.md)
- [@wae/open-ppt-skills](../open-ppt-skills/README.md)
