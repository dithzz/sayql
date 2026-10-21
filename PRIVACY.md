# Sayql Privacy Notice

Sayql is a desktop app. It has no Sayql account, no Sayql server and no telemetry: we don't collect, receive or store any of your data.

## What stays on your computer

- **Connections, settings and saved scripts** are kept in files in Sayql's folder in your user's application data.
- **Database passwords and AI API keys** are stored in your operating system's keychain (macOS Keychain, Windows Credential Manager or the Linux Secret Service), never in files.
- **Query results** are shown in the app and are not saved unless you copy or export them yourself.

## What Sayql sends, and where

Sayql sends data only to services you configure, and only when you use them:

| When | What is sent | Sent to |
| --- | --- | --- |
| You connect or run a query | Your login and your SQL | Your database server |
| You ask in plain English | Your question, the database's table and column names, types and keys, and (for follow-ups) the previous query | The AI provider you chose in Settings |
| A query fails and Sayql fixes it | The query and the database's error message | The AI provider you chose |
| An AI app uses Sayql through MCP | The schemas and query results it asks for | That AI app (for example Claude or Cursor) |
| Sayql checks for updates (when enabled) | The app version and your operating system | Sayql's update server |

The rows in your tables are never sent to an AI provider by the Ask box. With a local provider (Ollama or LM Studio), AI requests don't leave your computer.

## AI providers

What an AI provider does with your requests is governed by that provider's own terms and privacy policy. Check them before you connect a provider to a database with sensitive names or structures.

## Questions

Questions about this notice can be sent through the contact details on the Sayql website.
