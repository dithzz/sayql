# Changelog

All notable changes to SayQL are listed here. Downloads are on the
[releases page](https://github.com/dithzz/sayql-releases/releases).

## Unreleased
- **MySQL 5.7 connects:** with "Require SSL" off, SayQL now falls back to an unencrypted connection when
  the server's encryption can't be used, as MySQL's own clients and psql do. Before, MySQL 5.7 servers (and
  others offering only older TLS ciphers) couldn't be reached at all.
- **Clearer connection errors:** a wrong port or a stopped server now says "Could not reach host:port:
  Connection refused" right away, instead of "pool timed out" after 10 seconds. SSL problems say so, and how
  to connect without SSL.
- **PostgreSQL schema:** logins that can only `SELECT` (the setup we recommend) now see primary keys, which
  were missing for them. Materialized views are listed. Partitioned tables are listed once, with a row
  estimate that covers all their partitions, instead of once per partition.
- **AI and names that need quotes:** the schema SayQL sends to the AI (and MCP's `describe_schema`) now
  writes table and column names the way your database needs them: `"FirstName"` in PostgreSQL,
  `` `first name` `` in MySQL, `[Order Lines]` in SQL Server. Before, the AI copied such names unquoted and the
  query failed.
- **Readable database errors:** PostgreSQL errors no longer end in "at line 414" (a line in Postgres' own
  source code, not your query) and now include the DETAIL and HINT lines, e.g. which key already exists.
  SQL Server errors show the message, then the error number and the line of your query, instead of
  `Token error: '…' on server … executing …`.
- **Fixing SQL Server queries with AI works again:** the wrapped SQL Server message looked like one quoted
  value, so masking values before sending the error to the AI hid the whole message, column names
  included. Only the values are masked now, on every database.
- **Read-only mode covers effects a rollback keeps:** PostgreSQL functions that a read-only transaction
  allows but that outlast it now ask for confirmation, like any other write: creating or dropping
  replication slots, resetting statistics, and session advisory locks. So does MySQL's `GET_LOCK`.
- **`SHOW CREATE TABLE` runs in read-only mode** on MySQL and MariaDB; it no longer asks for confirmation.
- **Confirmed statements don't leave settings behind:** a `SET`, `USE`, lock or temporary table from a
  statement you confirmed no longer sticks to one of SayQL's pooled connections, where it changed the
  results of later queries at random (one query in four would run in the other database after a `USE`).
  Each confirmed statement now runs on a connection of its own.
- **SQL Server shows the rows a confirmed statement returns,** such as `INSERT ... OUTPUT inserted.*` or
  `DECLARE ...; SELECT ...`. Before, only "0 rows affected" was shown.
- **MySQL zero dates show as stored:** `0000-00-00` and dates with a zero month or day (`2024-00-15`) used
  to show as NULL. They now show as MySQL writes them.
- **MySQL geometry shows as WKT:** `POINT(1.5 -2)`, `POLYGON(...)` and the rest, instead of `<geometry>`.
- **SQL Server columns without a name** show as "(No column name)", as in SQL Server's own tools.
- **Cancel a running query:** a **Cancel** button next to **Run** stops a slow query on the database
  server itself (PostgreSQL, MySQL, MariaDB, SQL Server, SQLite), not just in SayQL. Queries from AI apps
  over MCP stop on their own after 2 minutes (exports after 10).
- **Scripts split the way your database reads them:** a `;` inside a MySQL `#` comment or `'it\'s'` string,
  or a Postgres `$tag1$` string, no longer splits a statement in two.
- **SQL Server `geometry` shows as WKT,** as `STAsText()` writes it, instead of bytes.
- **SQLite scripts over MCP** return the first result set, instead of every statement's rows under the
  first one's columns.

## 0.2.6 (2026-10-22)
- **Deleting a connection** also removes its password from the keychain item older versions saved it in.
- **API keys:** release builds no longer fall back to the `ANTHROPIC_API_KEY` environment variable; save the
  key in Settings.
- **Deleting a connection shows why when it fails** (e.g. keychain access denied); the connection and its tabs stay.

## 0.2.5 (2026-10-22)

- **Voice input:** click the mic in the Ask box, or press ⌘⇧M (Ctrl+Shift+M), and say your question. It
  becomes the question as if you'd typed it, keeping any `/command`; press ↵ to run. Speech is turned into
  text on your computer by Whisper, and audio is never saved or sent anywhere. The first time, the speech
  model (82 MB) downloads once from Hugging Face; remove or redownload it in Settings → Voice.
- **Safer saved connections:** if a saved file (connections, settings, scripts or skills) is damaged,
  SayQL no longer treats it as empty and erases it on the next save. It moves the file aside as
  `<name>.corrupt-<time>` and says where. Saves are now written in one step, so a crash mid-save can't
  leave a half-written file.
- **MCP: hand work to SayQL.** Two new tools for AI apps. `add_connection` opens the New connection dialog
  filled in; you type the password in SayQL. `open_query` opens SQL in a new tab for you to review and run.
  Passwords and writes never go through MCP, and nothing runs or saves on its own.
- **MCP: saved scripts.** `list_scripts` lists them with their SQL; `open_script` opens one in SayQL, unrun.
- **MCP: more rows.** `run_query` pages with `offset`, and `export_csv` saves up to 100,000 rows of a
  read-only query to a new CSV in Downloads without sending them to the AI app.
- **Privacy:** when SayQL fixes a failed query, values from your rows that the database quotes in its error
  (a duplicate email, a value that failed to convert) are masked before the error goes to your AI provider.
- **Stricter read-only check:** SayQL now reads each statement the way your database does (MySQL `#` comments,
  `/*! */` and backslash escapes; Postgres `E'...'` and `$$` strings), so a write can't hide inside them.
  `INTO OUTFILE`, `INTO DUMPFILE`, `NEXT VALUE FOR` and side-effect functions such as `nextval`, `dblink` and
  `pg_terminate_backend` now ask before running, and MCP refuses them.
- **Content Security Policy:** the app window now loads scripts only from SayQL itself and talks only to SayQL's
  own backend, so injected content can't load remote code or send data elsewhere.

## 0.2.5 (2026-10-22)
- **Security:** updated the SQL Server driver, which fixes a high-severity issue in a TLS library it used.
- **Under the hood:** newer database drivers (sqlx 0.9, tiberius 0.13), editor (Monaco 0.57) and icons.
- **Editor:** suggests your tables and columns as you type, plus find and replace (⌘F, ⌥⌘F) and comment
  toggling (⌘/).

## 0.2.4 (2026-10-22)

- **MCP server:** it has come with SayQL since 0.1.0 (the docs and website said "coming soon" by mistake).
  Set it up in Settings → AI apps (MCP). On Linux, Settings now warns that the AppImage's path changes on
  every launch; use the `.deb` or `.rpm` for MCP.

0.2.3 was not released: its build failed.

## 0.2.2 (2026-10-22)

- **One-line Ask box:** the tables you picked sit at the right end of the question, and the box stays one line
  until you type more.

## 0.2.1 (2026-10-22)

- **New logo and app icon:** "Say" with "QL" in a blue chip.

## 0.2.0 (2026-10-22)

- **Slash commands** in the Ask box: type `/` for `/followup`, `/new`, `/explain`, `/fix` and `/improve`.
- **Skills:** save an instruction once, like `/pii-safe` = "Never select email, phone or address columns", and add it
  to any question. Text only, stored on your computer, managed in Settings.
- **Settings is a full-window page** with a sidebar: AI provider, Skills, AI apps (MCP) and About.
- **Run scripts:** several statements run one after another, each with its own results tab. Writes still ask
  first, and the run stops at the first error.
- **Run selection:** select part of the SQL and press Run or ⌘↵ to run only that. A selection that cuts a
  statement in half runs the whole statement instead, so a `DELETE` never runs without its `WHERE`.
- **Setup guide** on first launch: pick your AI (the key is checked before saving), pick the demo or your own
  database, and run a first question. Reopen it from Settings.
- **Claude API keys are checked** in the setup guide before they're saved, so a typo shows up straight away.
- **Send feedback** from the status bar: a pre-filled public GitHub issue. Only the SayQL
  version and operating system are added.
- **Open source** under the Apache License 2.0, with contributor docs, a security policy and CI on pull requests.
- **Docs** at [sayql.dev/docs](https://sayql.dev/docs/): install, AI providers, databases, MCP,
  shortcuts and troubleshooting.

## 0.1.0 (2026-10-21)

The first release of SayQL Community Edition.

- **Ask in plain English:** SQL streams into the editor as you type. Simple requests are answered instantly on your
  machine, follow-ups edit the query you have, failed queries are fixed once automatically, and **Improve**
  reviews a query on request.
- **Any AI provider:** Claude, OpenAI, Google Gemini, OpenRouter, Mistral, Groq, DeepSeek, Ollama, LM Studio or any
  OpenAI-compatible API, with a fast model for drafts and a stronger one for fixes.
- **Databases:** PostgreSQL, MySQL / MariaDB, SQLite, SQL Server and Azure SQL, with every database on a server
  listed in the navigator and a built-in demo store.
- **Read-only by default:** statements are checked, reads run read-only in the database, and anything that could
  change data asks first.
- **Workbench:** tabs, saved scripts, and picking tables as AI context.
- **Secrets in the OS keychain**, in a single item, so macOS asks at most once after an update.
- **Automatic, signed updates** from the status bar.
