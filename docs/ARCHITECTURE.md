# Architecture

AskDB is a Tauri 2 desktop app: a React + TypeScript UI (`src/`) on top of a Rust backend (`src-tauri/`). The UI never talks to a database or the Claude API directly; it calls typed Tauri commands.

## Frontend layout (`src/`)

```
src/
  main.tsx              entry: global styles, dev-only mock backend, <App/>
  app/                  the window shell; composes features, owns no business logic
    App.tsx             ThemeProvider + bootstrap gate
    Workbench.tsx       layout: TopBar, Navigator | (SQL Editor / Results), StatusBar, dialogs
    TopBar.tsx, StatusBar.tsx, SqlWorkspace.tsx, useBootstrap.ts
  api/                  the only place that calls Tauri
    commands.ts         typed invoke() wrappers, one per Rust command
    events.ts           AI streaming event subscription
    types.ts            shapes mirrored from the Rust serde structs
  components/ui/        reusable, feature-agnostic primitives (Button, Dialog, Panel, SplitPane, TreeRow, Form…)
  features/             one folder per product area; each exports its public API from index.ts
    ask/                plain-English input, instant fast path, AI generation hook
    connections/        connection dialog, kinds metadata, useConnections
    editor/             Monaco setup (offline, themed) and SqlEditor
    navigator/          Database Navigator tree
    query/              useQueryRunner, write confirmation
    results/            virtualized DataGrid, ResultsPanel, CSV/format helpers
    schema/             dialect-aware SQL helpers (quoting, qualified names)
    settings/           settings dialog and state
    theme/              ThemeProvider (system/light/dark) and toggle
  hooks/                generic hooks (useLatest, usePersistentState)
  lib/                  generic utilities (cx, storage, errors)
  styles/               tokens.css (design tokens, light + dark) and base.css
  dev/                  development-only code (mock backend for browser preview)
```

### Rules

- **Dependencies point inward:** `app → features → components/ui, hooks, lib, api`. Features may use another feature's public `index.ts` (or a pure helper module such as `schema/sql.ts`), never its internals. `components/ui` never imports from `features` or `api`.
- **State lives in hooks, views stay presentational.** Each feature exposes a `useX` hook for its state and side effects; components receive data and callbacks as props.
- **One way to style:** CSS Modules next to the component (`Foo.tsx` + `Foo.module.css`), using only variables from `styles/tokens.css`. No hex colors or pixel font sizes outside `tokens.css` (the Monaco theme in `features/editor/monacoSetup.ts` is the single documented exception, because Monaco needs literal colors).
- **Theming:** `<html data-theme="light|dark">` switches the color tokens. New colors are added to both theme blocks in `tokens.css`.
- **Icons:** `lucide-react` only, 13–16px.
- **Naming:** components `PascalCase.tsx`, hooks `useCamelCase.ts`, helpers `camelCase.ts`, tests next to the code as `*.test.ts`.

## Backend layout (`src-tauri/src/`)

| File | Responsibility |
|---|---|
| `lib.rs` | Tauri commands and app state wiring |
| `db.rs` | connections, read-only execution, value decoding, schema introspection (PostgreSQL, MySQL, SQLite) |
| `schema.rs` | schema model and the compact text sent to the model |
| `ai.rs` | Claude Messages API streaming (raw HTTP + SSE) |
| `store.rs` | settings/connections JSON and OS keychain secrets |
| `demo.sql` | the bundled demo database |

## Testing

`pnpm test` runs the Vitest suite (pure frontend logic) and `cargo test` (database layer, against a generated SQLite demo database).

## Previewing the UI without the backend

`pnpm dev` and open http://localhost:1420 in a browser: `src/dev/mockBackend.ts` fakes the Rust commands with demo data, including streamed SQL. It is only loaded in development outside Tauri.
