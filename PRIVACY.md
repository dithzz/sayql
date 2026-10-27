# SayQL Privacy Notice

SayQL is a desktop app. It has no SayQL account, no SayQL server and no telemetry: we don't collect, receive or store any of your data.

## What stays on your computer

- **Connections, settings and saved scripts** are kept in files in SayQL's folder in your user's application data.
- **Database passwords, AI API keys and a SayQL Team sign-in** are stored in your operating system's keychain (macOS Keychain, Windows Credential Manager or the Linux Secret Service), never in files.
- **Query results** are shown in the app and are not saved unless you copy them yourself.
- **Agent mode** keeps, per connection, whether it's on and what it may send; database notes you save; and in
  each tab its last run's queries and answer (never its rows).
- **Voice input** is turned into text on your computer by a speech model (Whisper) that runs locally. Audio is
  kept in memory only while you talk, is never saved, and is never sent anywhere.

## What SayQL sends, and where

SayQL sends data only to services you configure, and only when you use them:

| When | What is sent | Sent to |
| --- | --- | --- |
| You connect or run a query | Your login and your SQL | Your database server |
| You ask in plain English | Your question; the database's table and column names, types, keys, enum values and row estimates; the current SQL for follow-ups, Explain, Fix and Improve; any skill you add | The AI provider you chose in Settings |
| A query fails and SayQL fixes it | The query and the database's error message, with values from your rows masked | The AI provider you chose |
| You use agent mode on a connection where you turned it on | Your question, the schema, saved database notes, the agent's queries and, depending on your setting for that connection, row counts only, counts and column summaries, or up to 20 rows per query with likely personal data masked | The AI provider you chose |
| An AI app uses SayQL through MCP | The schemas, saved database notes and query results it asks for (never the values of columns you marked never send) | That AI app (for example Claude or Cursor) |
| You sign in to a SayQL Team server and use a team connection | Your invite code (once), requests for team connections and their schemas, and your queries with the questions they answer and where they came from (Ask box, editor or agent mode) | Your company's SayQL server, at the address you entered |
| You use the Ask box or agent mode on a team connection, when your team provides AI | Your question, the schema, saved database notes and, in agent mode if allowed, masked rows or summaries (what the AI provider would otherwise get), labelled with where it came from and the connection | Your company's SayQL server, which forwards it to your company's AI provider and keeps only token counts |
| You turn on voice input, once | A download request for the speech model (about 82 MB), checked against a fixed SHA-256 before use. No audio is sent | Hugging Face (huggingface.co/ggerganov/whisper.cpp) |
| SayQL checks for updates, each time it starts | The app version and your operating system | GitHub (github.com/dithzz/sayql-releases) |

When you are signed in to a team server, queries and questions on its connections go to that server (run by your company), which records them in its audit log. Without signing in, SayQL never contacts a team server. Your own connections never go there either way.

When your team also provides AI (team AI), AI requests on team connections go to your company's SayQL server instead of your own AI provider: your question, the schema and, in agent mode if your settings and your team admin allow it, masked rows. The server forwards them to your company's AI provider and stores only token counts, never the prompt or the answer. Nothing reaches SayQL itself. Your own connections keep using your own AI provider.

**SayQL AI credits** are the exception, and only if your team admin chooses them: instead of a company AI key, the team server then uses a prepaid SayQL credit key, and its AI provider is SayQL's endpoint at `sayql.dev/api/ai`. Those requests (the question, the schema and, in agent mode if allowed, masked rows) pass through SayQL's service on their way to Anthropic, with SayQL's own Anthropic key. SayQL stores the model and token counts of each request against your team's balance, never the prompt or the answer. Anthropic handles the request under its commercial terms.

The Ask box never sends rows from your tables to an AI provider. Agent mode can, only on connections where you turn it on; it is off by default. Masking in agent mode is best effort: a free-text column can hold anything, so mark such columns "never send", or keep results to counts. With "Ask before each send", you see exactly what would go before it goes. With a local provider (Ollama or LM Studio), AI requests don't leave your computer.

## The website

The SayQL website (sayql.dev) uses Vercel Web Analytics to count page views and where visitors come
from. It uses no cookies, doesn't track you across sites, and stores no personal data. The app itself sends
nothing to it.

If you join the SayQL for teams waitlist, the website stores the email, interest and team size you enter
(not your IP address), and uses them only to contact you about SayQL for teams. To be removed, email
hello@azenvoc.com.

A SayQL for teams server's admin page offers an optional **Tell SayQL about your trial** form. Only if
the admin fills it in and clicks Send, their browser sends the company, email and team size they typed,
and the server's version number, to sayql.dev, which stores them (not the IP address) and uses them
only to help with the trial. The server itself never contacts SayQL. To be removed, email
hello@azenvoc.com.

Running the installer (`sayql.dev/install`) adds one to a count of installs per day. Nothing else
about the request is kept.

## Feedback

**Feedback** in the status bar opens a short form, then a pre-filled public GitHub issue in your browser.
Your text is filled in, plus your SayQL version and operating system for feedback and bug reports.
Nothing is sent until you post it.

## AI providers

What an AI provider does with your requests is governed by that provider's own terms and privacy policy. Check them before you connect a provider to a database with sensitive names or structures.

## Questions

Questions about this notice: hello@azenvoc.com.
