# AskDB

An AI-first desktop database client. Type what you want in plain English; SQL appears as you type; press Enter to run it.

- **Databases:** PostgreSQL, MySQL / MariaDB, SQLite
- **Stack:** Tauri 2 (Rust) + React + Monaco editor. Database drivers use `sqlx`. All dependencies are MIT/Apache-2.0 licensed, so the app can be sold as a closed-source product.
- **AI:** Claude API (Opus 5.5 by default; Sonnet 5.5 / Haiku 5.5 selectable in Settings).

## How it stays fast

1. The schema is read once on connect and cached.
2. Simple requests ("show customers", "how many orders", "latest 20 reviews") are answered instantly from the schema with no AI call (`src/features/ask/fastPath.ts`).
3. Repeated questions are served from an in-memory cache.
4. Everything else goes to Claude 250 ms after you stop typing. The schema sits in a prompt-cached system block, so only your question is processed fresh. The SQL streams into the editor, and each new keystroke cancels the request before it.

## Safety

- Only the schema (table/column names, types, keys, enum values) and your question are sent to Claude. Table data never is.
- Read queries run inside a read-only transaction (`BEGIN READ ONLY` / `START TRANSACTION READ ONLY`) or, for SQLite, on a connection opened read-only, so the database itself rejects writes.
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

To work on the UI without the Rust backend, run `pnpm dev` and open http://localhost:1420 in a browser; a mock backend serves demo data.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the folder structure and code conventions.

### Integration tests against PostgreSQL and MySQL

`pnpm test` covers SQLite only. The PostgreSQL and MySQL tests in `src-tauri/src/db/integration_tests.rs` are skipped unless you point them at a server:

```bash
docker run -d --name askdb-test-pg -e POSTGRES_PASSWORD=<password> -e POSTGRES_DB=askdb -p 55416:5432 postgres:16
docker run -d --name askdb-test-mysql -e MYSQL_ROOT_PASSWORD=<password> -p 33806:3306 mysql:8
cd src-tauri
ASKDB_TEST_PG_URL=postgres://postgres:<password>@localhost:55416/askdb \
ASKDB_TEST_MYSQL_URL=mysql://root:<password>@localhost:33806 \
cargo test --lib integration
```

The URLs take the form `scheme://user:password@host:port/database` (no percent-encoding). The tests drop and recreate the Postgres schemas `askdb_it` and `askdb_it_inv` and the MySQL database `askdb_it`, so use a throwaway server. MySQL needs a few seconds after `docker run` before it accepts connections.
