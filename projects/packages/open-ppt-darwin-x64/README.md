# @wae/open-ppt-darwin-x64

Prebuilt macOS x64 native addon for [`@wae/open-ppt`](../open-ppt/README.md). **Not a public import target.** It is installed automatically as an optional dependency on matching `os` and `cpu`; application code imports `@wae/open-ppt`.

## Example

```ts
import { OpenPPT } from '@wae/open-ppt';

const ppt = new OpenPPT();
const deck = await ppt.create({ title: 'Quarterly review', size: '16:9' });
await deck.export({ format: 'pptx', target: 'review.pptx' });
```
