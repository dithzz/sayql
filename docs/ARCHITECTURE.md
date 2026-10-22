# Architecture

SayQL is a Tauri 2 desktop app: a React + TypeScript UI (`src/`) on top of a Rust backend (`src-tauri/`). The UI never talks to a database or an AI provider directly; it calls typed Tauri commands.

## Frontend layout (`src/`)

```
src/
  main.tsx              entry: global styles, dev-only mock backend, <App/>
  app/                  the window shell; composes features, owns no business logic
    App.tsx             ThemeProvider + bootstrap gate
    Workbench.tsx       layout: TopBar, Navigator | tabs, StatusBar, dialogs; the navigator follows the active tab
    QueryTab.tsx        one editor tab: Ask bar + SQL editor over results, bound to one database
    TopBar.tsx, StatusBar.tsx, SqlWorkspace.tsx, useBootstrap.ts
  api/                  the only place that calls Tauri
    commands.ts         typed invoke() wrappers, one per Rust command
    events.ts           AI streaming event subscription
    types.ts            shapes mirrored from the Rust serde structs
  components/ui/        reusable, feature-agnostic primitives (Button, Dialog, Panel, SplitPane, TabBar, TreeRow, Form,
                        ContextMenu, BrandMark…)
  features/             one folder per product area; each exports its public API from index.ts
    ask/                plain-English input, instant fast path, AI generation hook
    connections/        connection dialog, kinds metadata, useConnections
    editor/             Monaco setup (offline, themed) and SqlEditor
    navigator/          Database Navigator tree and its right-click menus
    onboarding/         first-run setup guide and starter questions
    query/              useQueryRunner, write confirmation
    results/            virtualized DataGrid, ResultsPanel, CSV/format helpers
    schema/             dialect-aware SQL helpers (quoting, qualified names, SQL templates)
    scripts/            saved scripts, save and close-tab dialogs
    settings/           settings dialog (AI provider, models, MCP) and state
    tabs/               open editor tabs (persisted), each bound to a (connection, database) target
    theme/              ThemeProvider (system/light/dark) and toggle
    updates/            update check and install from the status bar
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

```
src-tauri/src/
  lib.rs                plugin + state + command registration only
  mcp.rs                sayql-mcp: MCP server over stdio (read-only tools), reusing state/db/schema
  bin/sayql-mcp.rs      its entry point
  error.rs              AppError / AppResult: the single error type; serializes to a message for the UI
  state.rs              AppState: store, open connections, the running AI generation
  commands/             Tauri commands, one file per UI area (thin: validate, call modules, map results)
    connections.rs      init, save/delete/test/open connection, refresh schema
    query.rs            run_query (write confirmation gate)
    generation.rs       generate_sql, cancel_generation
    settings.rs         save_settings, the settings view sent to the UI
    voice.rs            voice_start/stop/cancel, model download and removal (events: voice://level, voice://progress)
  db/                   database access, no Tauri types
    mod.rs              Kind, ConnConfig, DbPool, QueryResult, public API
    connect.rs          pools per database kind
    mssql_pool.rs       SQL Server connection (tiberius), Azure redirects, host,port parsing
    classify.rs         is_read_only: reading first keyword and no write keyword anywhere
    execute.rs          run_query: read-only transactions, row cap, column headers
    decode/             driver values → JSON for display (postgres binary wire format, mysql, sqlite, mssql)
    introspect/         schema reading per database kind, shared assembly in mod.rs
    demo.rs, demo.sql   the bundled demo database
    tests.rs            SQLite end-to-end tests
    integration_tests.rs  PostgreSQL/MySQL/SQL Server tests (opt-in via env vars, see README)
  ai/                   English → SQL over any AI provider, no knowledge of Tauri commands
    mod.rs              stream_sql (picks the client per Endpoint) + event payloads
    providers.rs        the provider catalog: Claude plus OpenAI-compatible presets
    anthropic.rs        Claude Messages API, prompt caching, API error messages
    openai.rs           OpenAI-compatible Chat Completions (OpenAI, Gemini, Ollama, …), model lists
    models.rs           Claude's models and per-model request options
    prompt.rs           the English → SQL prompt, shared by every provider
    sse.rs              server-sent-event parser
  schema/               schema model (mod.rs) and compact prompt rendering (render.rs)
  store/                settings/connections JSON (mod.rs) and keychain secrets (secrets.rs)
  voice/                voice input, all on this computer, no Tauri types
    model.rs            the Whisper model: pinned URL and SHA-256, download, path in app data
    audio.rs            microphone capture with cpal, mono 16 kHz resampling
    transcribe.rs       whisper.cpp (whisper-rs) speech to text
```

### Rules

- **Commands are thin.** Business logic lives in `db/`, `ai/`, `schema/`, `store/` and `voice/`, which never import Tauri command types; only `commands/` and `state.rs` know about Tauri state.
- **Errors:** functions return `AppResult<T>`. Use `?` on sqlx/io/keychain/JSON errors (converted by `From`), and `"message".into()` for messages written for the user. A message that mentions "Settings" makes the UI offer an Open Settings button.
- **Secrets** only go through `store::secrets`; they are never written to JSON or sent to the UI.
- **Tests** sit next to the code (`#[cfg(test)] mod tests`).

## Testing

`pnpm test` runs the Vitest suite (pure frontend logic) and `cargo test --lib` (SQL classification, SSE parsing, API error messages, settings validation, and an end-to-end run against a generated SQLite demo database). The PostgreSQL/MySQL integration tests run when their env vars are set (see README).

## Previewing the UI without the backend

`pnpm dev` and open http://localhost:1420 in a browser: `src/dev/mockBackend.ts` fakes the Rust commands with demo data, including streamed SQL. It is only loaded in development outside Tauri.

## Outside the app

```
site/                 the website (landing page, built docs), deployed to Vercel
docs-site/            docs source: pages/*.html and the template the docs build uses
scripts/              build_docs.py (docs) and third_party_notices.py (credits shipped with the app)
.github/              CI, the release workflow, issue and PR templates, README images
```
