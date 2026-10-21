# AskDB

An AI-first desktop database client. Type what you want in plain English; SQL appears as you type; press Enter to run it.

- **Databases:** PostgreSQL, MySQL / MariaDB, SQLite, Microsoft SQL Server / Azure SQL
- **Stack:** Tauri 2 (Rust) + React + Monaco editor. Database drivers are `sqlx` and `tiberius` (SQL Server). All dependencies are MIT/Apache-2.0 licensed, so the app can be sold as a closed-source product.
- **AI:** Claude API (Opus 5.5 by default; Sonnet 5.5 / Haiku 5.5 selectable in Settings).

## How it stays fast

1. The schema is read once on connect and cached.
2. Simple requests ("show customers", "how many orders", "latest 20 reviews") are answered instantly from the schema with no AI call (`src/features/ask/fastPath.ts`).
3. Repeated questions are served from an in-memory cache.
4. Everything else goes to Claude 250 ms after you stop typing. The schema sits in a prompt-cached system block, so only your question is processed fresh. The SQL streams into the editor, and each new keystroke cancels the request before it.

## Safety

- Only the schema (table/column names, types, keys, enum values) and your question are sent to Claude. Table data never is.
- A statement counts as read-only only if it starts with a reading keyword and contains no write keyword (outside strings and comments). Everything else needs confirmation.
- Read queries also run inside a read-only transaction (`BEGIN READ ONLY` / `START TRANSACTION READ ONLY`) or, for SQLite, on a connection opened read-only, so the database itself rejects writes. SQL Server has no read-only mode: reads run in a transaction that is always rolled back. For production SQL Servers, connect with a login that only has `db_datareader`.
- INSERT/UPDATE/DELETE/DDL require an explicit confirmation.
- Passwords and the API key are stored in the OS keychain. Connection details go in `connections.json` in the app config dir.

## Use AskDB from Claude (MCP)

`askdb-mcp` is an MCP server (stdio) that lets Claude Code or Claude Desktop use the connections saved in AskDB:
`list_connections`, `list_databases`, `describe_schema` (with `search` for large databases) and `run_query`.

- **Read-only.** Queries go through the same checks as the app's read-only mode; anything that could change data
  is refused and must be run in the AskDB app.
- **Query results are sent to Claude**, unlike the Ask box, which only sends table and column names.
- Passwords come from the OS keychain. On macOS, the first use asks to let `askdb-mcp` read AskDB's keychain items.

Settings → "Use AskDB from Claude" shows the exact command and config. For Claude Code:

```bash
claude mcp add --scope user askdb -- /path/to/askdb-mcp
```

In development the binary is `src-tauri/target/debug/askdb-mcp` (`cargo build --bin askdb-mcp`). Release bundles
must ship it next to the app executable (Tauri `bundle.externalBin`; see docs/RELEASING.md).

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

### Integration tests against PostgreSQL, MySQL and SQL Server

`pnpm test` covers SQLite only. The PostgreSQL and MySQL tests in `src-tauri/src/db/integration_tests.rs` are skipped unless you point them at a server:

```bash
docker run -d --name askdb-test-pg -e POSTGRES_PASSWORD=<password> -e POSTGRES_DB=askdb -p 55416:5432 postgres:16
docker run -d --name askdb-test-mysql -e MYSQL_ROOT_PASSWORD=<password> -p 33806:3306 mysql:8
docker run -d --platform linux/amd64 --name askdb-test-mssql -e ACCEPT_EULA=Y -e 'MSSQL_SA_PASSWORD=<password>' -p 14334:1433 mcr.microsoft.com/mssql/server:2022-latest
cd src-tauri
ASKDB_TEST_PG_URL=postgres://postgres:<password>@localhost:55416/askdb \
ASKDB_TEST_MYSQL_URL=mysql://root:<password>@localhost:33806 \
ASKDB_TEST_MSSQL_URL='mssql://sa:<password>@localhost:14334/master' \
cargo test --lib integration
```

The URLs take the form `scheme://user:password@host:port/database` (no percent-encoding). The tests drop and recreate the Postgres schemas `askdb_it` and `askdb_it_inv` and the MySQL and SQL Server databases `askdb_it`, so use throwaway servers. MySQL needs a few seconds after `docker run` before it accepts connections.
