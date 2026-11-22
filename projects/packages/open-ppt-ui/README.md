# @wae/open-ppt-ui

Desktop shell for OpenPPT Studio. This private package contains the VMZ static UI that is staged into the `@wae/open-ppt` GUI asset; it is not published on its own.

## Usage

```bash
open-ppt gui
```

The window talks to the native host over `chrome.webview.postMessage` JSON-RPC (`ready`, `capabilities`, `deck.create`, `deck.open`, `deck.save`, `deck.inspect`, `deck.export`, `deck.close`, `close`). Without a WebView host it shows an empty state and a preview notice; it never fabricates a deck.
