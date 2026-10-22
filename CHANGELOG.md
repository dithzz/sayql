# Changelog

All notable changes to SayQL are listed here. Downloads are on the
[releases page](https://github.com/dithzz/sayql-releases/releases).

## Unreleased

- **Slash commands** in the Ask box: type `/` for `/followup`, `/new`, `/explain`, `/fix` and `/improve`.
- **Skills:** save an instruction once, like `/pii-safe` = "Never select email, phone or address columns", and add it
  to any question. Text only, stored on your computer, managed in Settings.
- **Setup guide** on first launch: pick your AI (the key is checked before saving), pick the demo or your own
  database, and run a first question. Reopen it from Settings.
- **Claude API keys are checked** in the setup guide before they're saved, so a typo shows up straight away.
- **Send feedback** from the status bar: a pre-filled public GitHub issue. Only the SayQL
  version and operating system are added.
- **Open source** under the Apache License 2.0, with contributor docs, a security policy and CI on pull requests.
- **Docs** at [sayql.vercel.app/docs](https://sayql.vercel.app/docs/): install, AI providers, databases, MCP,
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
