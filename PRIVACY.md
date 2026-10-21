# SayQL Privacy Notice

SayQL is a desktop app. It has no SayQL account, no SayQL server and no telemetry: we don't collect, receive or store any of your data.

## What stays on your computer

- **Connections, settings and saved scripts** are kept in files in SayQL's folder in your user's application data.
- **Database passwords and AI API keys** are stored in your operating system's keychain (macOS Keychain, Windows Credential Manager or the Linux Secret Service), never in files.
- **Query results** are shown in the app and are not saved unless you copy them yourself.

## What SayQL sends, and where

SayQL sends data only to services you configure, and only when you use them:

| When | What is sent | Sent to |
| --- | --- | --- |
| You connect or run a query | Your login and your SQL | Your database server |
| You ask in plain English | Your question, the database's table and column names, types and keys, and (for follow-ups) the previous query | The AI provider you chose in Settings |
| A query fails and SayQL fixes it | The query and the database's error message | The AI provider you chose |
| An AI app uses SayQL through MCP | The schemas and query results it asks for | That AI app (for example Claude or Cursor) |
| SayQL checks for updates, each time it starts | The app version and your operating system | GitHub (github.com/dithzz/sayql-releases) |

The rows in your tables are never sent to an AI provider by the Ask box. With a local provider (Ollama or LM Studio), AI requests don't leave your computer.

## The website

The SayQL website (sayql.vercel.app) uses Vercel Web Analytics to count page views and where visitors come
from. It uses no cookies, doesn't track you across sites, and stores no personal data. The app itself sends
nothing to it.

## Feedback

**Send feedback** in the app opens a pre-filled public GitHub issue in your browser. Only your
SayQL version and operating system are filled in. Nothing else is sent unless you type it.

## AI providers

What an AI provider does with your requests is governed by that provider's own terms and privacy policy. Check them before you connect a provider to a database with sensitive names or structures.

## Questions

Questions about this notice can be sent through the contact details on the SayQL website.
