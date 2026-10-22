# Show HN

Post at https://news.ycombinator.com/submit. Use the **URL** field (not text), then add the first comment right away.

**Title** (80 characters max):

```
Show HN: SayQL – Plain English to SQL, read-only by default (open source)
```

**URL:**

```
https://github.com/dithzz/sayql
```

The repo is the better link for HN: code first, website one click away.

## First comment

```
Hi HN, I'm Adith. I built SayQL, a desktop app (macOS, Windows, Linux) that turns plain English into SQL as you type. It's free and Apache-2.0.

Why: a lot of people need answers from a production database but don't write SQL every day. Asking an engineer is slow, and a chatbot doesn't know your schema and can't run anything safely. I wanted something you could point at production without worrying.

How it works:
- It reads your schema once (tables, columns, keys) and writes SQL against your real names and joins.
- SQL streams into the editor as you type. Simple requests are answered locally; follow-ups ("only Canada", "sort by date") edit the query you have; if the database rejects a query, it tries one fix.
- Bring your own key: Claude, OpenAI, Gemini, OpenRouter, Mistral, Groq, DeepSeek, or any OpenAI-compatible API. Or Ollama / LM Studio, so nothing leaves your machine.
- Databases: PostgreSQL, MySQL/MariaDB, SQLite, SQL Server, Azure SQL.

Safety, since that's the point:
- Every statement is checked. It counts as a read only if it starts with a read keyword and has no write keyword outside strings and comments.
- Reads then run inside a read-only transaction (BEGIN READ ONLY / START TRANSACTION READ ONLY, or a read-only SQLite connection), so the database rejects a write even if my check is wrong.
- SQL Server has no read-only transaction, so reads there run in a transaction that's always rolled back. For production SQL Server, use a db_datareader login.
- Anything that could change data stops and shows you the exact statement first.
- The AI gets your question and the schema. Never table rows.
- No account, no server of mine, no telemetry. Passwords and keys live in the OS keychain.

Also in there: slash commands (/explain, /fix, /improve), saved "skills" like /pii-safe, and an MCP server so Claude or Cursor can query your saved connections with the same read-only checks.

Stack: Tauri 2 (Rust), React, Monaco, sqlx and tiberius.

Known gaps, so you don't have to find them:
- No SSH tunnels yet.
- Builds aren't code-signed yet, so macOS and Windows ask you to confirm the first launch.
- No query history or file export yet.

Feedback I'd most like:
- Can you break the read-only check? Statements that slip through, or false alarms.
- Where does the SQL go wrong on your schema?
- Which of the gaps above matters most to you?

Website: https://sayql.dev
Downloads: https://github.com/dithzz/sayql-releases/releases/latest
```

## Notes

- Show HN rules: it must be something people can try now (it is: free download, built-in demo database, no signup).
- Don't ask anyone to upvote, and don't share a direct link asking for votes. HN penalizes it.
- Stay in the thread for the first 2–3 hours. See [checklist.md](checklist.md).
