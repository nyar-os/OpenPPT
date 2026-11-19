# open-ppt-ui

VMZ static UI for the OpenPPT desktop window. `vmz build . --profile static`
emits `dist/static`, which the repository staging step copies to
`projects/packages/open-ppt/dist/gui` for the native WebView host.

```bash
pnpm --filter @wae/open-ppt-ui run check   # vmz check
pnpm --filter @wae/open-ppt-ui run build   # vmz build --profile static
pnpm --filter @wae/open-ppt-ui run serve   # local preview
```

The page talks to the host over `chrome.webview.postMessage` JSON-RPC
(`ready`, `deck.create`, `deck.open`, `deck.slide`, `deck.asset`, `deck.edit`,
`deck.undo`, `deck.redo`, `deck.save`, `deck.close`, `close`). Without a
WebView host it falls back to a built-in preview slide so the layout can be
inspected in a plain browser.
