# Sayql

An AI-first desktop database client. Type what you want in plain English; SQL appears as you type; press Enter to run it.

- **Databases:** PostgreSQL, MySQL / MariaDB, SQLite, Microsoft SQL Server / Azure SQL
- **Stack:** Tauri 2 (Rust) + React + Monaco editor. Database drivers are `sqlx` and `tiberius` (SQL Server). All dependencies are MIT/Apache-2.0 licensed, so the app can be sold as a closed-source product.
- **AI:** any provider. Claude is built in (Haiku 5.5 drafts, Opus 5.5 fixes, by default). OpenAI, Google Gemini, OpenRouter, Mistral, Groq, DeepSeek, Ollama, LM Studio and any other OpenAI-compatible API (with its base URL) work through one client. Ollama and LM Studio run on your computer, so nothing leaves it.

## How it stays fast

1. The schema is read once on connect and cached.
2. Simple requests ("show customers", "how many orders", "latest 20 reviews") are answered instantly from the schema with no AI call (`src/features/ask/fastPath.ts`).
3. Repeated questions are served from an in-memory cache.
4. Everything else goes to the AI provider 250 ms after you stop typing. The schema sits in a fixed prefix (a cached system block on Claude; automatic prefix caching where the provider has it), so only your question is processed fresh. The SQL streams into the editor, and each new keystroke cancels the request before it.

## Safety

- Only the schema (table/column names, types, keys, enum values) and your question are sent to the AI provider you chose. Table data never is. API keys live in the OS keychain, one entry per provider.
- A statement counts as read-only only if it starts with a reading keyword and contains no write keyword (outside strings and comments). Everything else needs confirmation.
- Read queries also run inside a read-only transaction (`BEGIN READ ONLY` / `START TRANSACTION READ ONLY`) or, for SQLite, on a connection opened read-only, so the database itself rejects writes. SQL Server has no read-only mode: reads run in a transaction that is always rolled back. For production SQL Servers, connect with a login that only has `db_datareader`.
- INSERT/UPDATE/DELETE/DDL require an explicit confirmation.
- Passwords and the API key are stored in the OS keychain. Connection details go in `connections.json` in the app config dir.

## Use Sayql from Claude (MCP)

`sayql-mcp` is an MCP server (stdio) that lets Claude Code or Claude Desktop use the connections saved in Sayql:
`list_connections`, `list_databases`, `describe_schema` (with `search` for large databases) and `run_query`.

- **Read-only.** Queries go through the same checks as the app's read-only mode; anything that could change data
  is refused and must be run in the Sayql app.
- **Query results are sent to Claude**, unlike the Ask box, which only sends table and column names.
- Passwords come from the OS keychain. On macOS, the first use asks to let `sayql-mcp` read Sayql's keychain items.

Settings → "Use Sayql from Claude" shows the exact command and config. For Claude Code:

```bash
claude mcp add --scope user sayql -- /path/to/sayql-mcp
```

In development the binary is `src-tauri/target/debug/sayql-mcp` (`cargo build --bin sayql-mcp`). Release bundles
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

Pick a throwaway password for the local test containers (SQL Server needs 8+ characters with upper and lower case,
a digit and a symbol) and keep it out of the repository:

```bash
export SAYQL_TEST_DB_PASSWORD='<throwaway password>'
docker run -d --name sayql-test-pg -e POSTGRES_PASSWORD="$SAYQL_TEST_DB_PASSWORD" -e POSTGRES_DB=sayql -p 55416:5432 postgres:16
docker run -d --name sayql-test-mysql -e MYSQL_ROOT_PASSWORD="$SAYQL_TEST_DB_PASSWORD" -p 33806:3306 mysql:8
docker run -d --platform linux/amd64 --name sayql-test-mssql -e ACCEPT_EULA=Y -e MSSQL_SA_PASSWORD="$SAYQL_TEST_DB_PASSWORD" -p 14334:1433 mcr.microsoft.com/mssql/server:2022-latest
cd src-tauri
SAYQL_TEST_PG_URL="postgres://postgres:$SAYQL_TEST_DB_PASSWORD@localhost:55416/sayql" \
SAYQL_TEST_MYSQL_URL="mysql://root:$SAYQL_TEST_DB_PASSWORD@localhost:33806" \
SAYQL_TEST_MSSQL_URL="mssql://sa:$SAYQL_TEST_DB_PASSWORD@localhost:14334/master" \
cargo test --lib integration
```

The URLs take the form `scheme://user:password@host:port/database` (no percent-encoding). The tests drop and recreate the Postgres schemas `sayql_it` and `sayql_it_inv` and the MySQL and SQL Server databases `sayql_it`, so use throwaway servers. MySQL needs a few seconds after `docker run` before it accepts connections.

## License

Sayql Community Edition is free for personal and commercial use, under the terms in [LICENSE.md](LICENSE.md). [PRIVACY.md](PRIVACY.md) says what Sayql sends and where. Open-source components are credited in `THIRD-PARTY-NOTICES.txt`; regenerate it before each release with `python3 scripts/third_party_notices.py`. All three files ship inside the app bundle.
