# AskDB

An AI-first desktop database client. Type what you want in plain English; SQL appears as you type; press Enter to run it.

- **Databases:** PostgreSQL, MySQL / MariaDB, SQLite
- **Stack:** Tauri 2 (Rust) + React + Monaco editor. Database drivers use `sqlx`. All dependencies are MIT/Apache-2.0 licensed, so the app can be sold as a closed-source product.
- **AI:** Claude API (Opus 5.5 by default; Sonnet 5.5 / Haiku 5.5 selectable in Settings).

## How it stays fast

1. The schema is read once on connect and cached.
2. Simple requests ("show customers", "how many orders", "latest 20 reviews") are answered instantly from the schema with no AI call (`src/fastpath.ts`).
3. Repeated questions are served from an in-memory cache.
4. Everything else goes to Claude 250 ms after you stop typing. The schema sits in a prompt-cached system block, so only your question is processed fresh. The SQL streams into the editor, and each new keystroke cancels the request before it.

## Safety

- Only the schema (table/column names, types, keys, enum values) and your question are sent to Claude. Table data never is.
- Read queries run inside a read-only transaction (`BEGIN READ ONLY` / `START TRANSACTION READ ONLY` / `PRAGMA query_only`), so the database itself rejects writes.
- INSERT/UPDATE/DELETE/DDL require an explicit confirmation.
- Passwords and the API key are stored in the OS keychain. Connection details go in `connections.json` in the app config dir.

## Development

```bash
pnpm install
pnpm tauri dev      # run the app
pnpm test           # frontend + backend tests
pnpm tauri build    # release bundle
```

A demo SQLite store database is created on first launch.
