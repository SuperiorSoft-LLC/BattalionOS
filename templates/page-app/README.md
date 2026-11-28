# A BattalionOS page app (template)

React + TypeScript + Vite and [`@battalionos/app-api`](../../docs/apps/app-api.md). The demo lists and edits files under the
OS root (`files.read`, `files.write`) and remembers the last folder in the app's own store.

```
pnpm install
pnpm dev          # http://127.0.0.1:5173: in a plain browser it runs on the local backend (no OS)
pnpm build        # dist-app/ = app.json + ui/
pnpm test         # the demo's calls and the packer
pnpm package      # out/: .bosapp per platform, .minisig, feed.json, catalog-entry.json, public.key
```

In BattalionOS with live reload: `scripts/bos-dev.ps1 -App <this folder>\dist-app -AppUrl http://127.0.0.1:5173`.
Signing takes the key from `BOS_APP_SIGNING_KEY` (+ `BOS_APP_SIGNING_KEY_PASSWORD`) only; without it, a throwaway key.

The walkthrough: [docs/apps/make-your-own-app.md](../../docs/apps/make-your-own-app.md).
