# Product Hunt

Schedule at https://www.producthunt.com/posts/new. Launches go live at 12:01 AM Pacific.

**Name:** SayQL

**Tagline** (60 max):

```
Plain English to SQL, safe for production databases
```

**Description** (260 max):

```
Say what you need. It knows your schema. SayQL is a free, open-source desktop app that turns plain English into SQL as you type. Read-only by default: the database enforces it, and anything that could change data asks first. Bring your own AI key.
```

**Links:**
- Website: https://sayql.dev
- Download: https://github.com/dithzz/sayql-releases/releases/latest
- Source: https://github.com/dithzz/sayql

**Pricing:** Free. **Open source:** yes.

## Topics

Pick 3 from Product Hunt's picker:

1. Developer Tools
2. Open Source
3. Artificial Intelligence

(Use "SQL" or "Databases" instead of one of these if the picker offers it.)

## Gallery

Images from `.github/assets/` (light versions), plus `media/explainer.mp4` as the video (upload to YouTube first; PH takes a YouTube link).

| # | Image | Caption |
| --- | --- | --- |
| 1 | `hero-light.png` | Ask in plain English. Get SQL and results. |
| 2 | `feature-ask-light.png` | Pick tables as context. It writes against your real schema. |
| 3 | `feature-write-confirm-light.png` | Anything that could change data asks first. |
| 4 | `feature-setup-ai-light.png` | Your AI, your key. Or run a local model. |
| 5 | `social-preview.png` | Free and open source. macOS, Windows, Linux. |

PH gallery images are 1270×760. Check each one crops well before launch.

## Maker's first comment

```
Hi Product Hunt, I'm Adith, and I made SayQL.

Lots of people need answers from a production database but don't write SQL every day. Asking an engineer is slow. A chatbot doesn't know your schema and can't run anything safely.

SayQL fixes that:
- Type what you want. SQL appears as you type, using your real tables and joins.
- Read-only by default. Every statement is checked, reads run in a read-only transaction the database enforces, and anything that could change data shows you the statement first.
- The AI sees your schema, never your rows.
- Use your own key for Claude, OpenAI, Gemini and others, or a local model with Ollama or LM Studio.
- PostgreSQL, MySQL/MariaDB, SQLite, SQL Server and Azure SQL.

It's free and Apache-2.0. There's a demo database built in, so you can try it without connecting anything.

Not there yet: SSH tunnels, signed builds, query history and export. Tell me which one you need first.
```
