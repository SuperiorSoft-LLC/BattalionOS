# @battalionos/app-api

The typed client for BattalionOS apps: files under the OS root, the app's own store, notices, badges, windows and pickers,
with typed errors and a local backend for a plain browser. Docs: [docs/apps/app-api.md](../../docs/apps/app-api.md).
Start an app: [docs/apps/make-your-own-app.md](../../docs/apps/make-your-own-app.md).

```
pnpm install
pnpm test        # vitest: client over a fake bridge, errors, local backend, generated types fresh
pnpm typecheck
```
