# Reddit: r/PostgreSQL

## Rules check (do this before posting)

I couldn't read r/PostgreSQL's rules automatically: Reddit blocks automated access, and no copy of the rules turned up in search. Before posting:

1. Open https://www.reddit.com/r/PostgreSQL/about/rules and read every rule.
2. Look for: a self-promotion or "tools" rule, a required flair (often "Tools" or "Projects"), and any pinned thread for new projects.
3. If promotion isn't allowed as a post, skip this sub or ask the mods first by modmail.

General Reddit norms that apply regardless:
- Say plainly that you made it.
- Keep it technical; this sub cares about how Postgres is used, not features.
- Don't post the same day as r/SQL.

## Title

```
Plain-English-to-SQL desktop app that runs every read in BEGIN READ ONLY (open source, looking for holes)
```

## Body

```
I made SayQL, a free Apache-2.0 desktop app that turns plain English into SQL against your schema. Posting here because the Postgres side is where I want scrutiny.

How it handles Postgres:
- It reads the schema once (tables, columns, types, foreign keys) and caches it. On big schemas it sends only the tables relevant to the question.
- Every statement is checked first: it's a read only if it starts with a read keyword and has no write keyword outside strings and comments.
- Reads then run inside BEGIN READ ONLY, so Postgres itself rejects writes if the check gets something wrong.
- Anything else stops and shows you the exact statement before it runs.
- The AI sees the schema, never rows. Bring your own key, or point it at Ollama so nothing leaves the machine.
- The navigator lists every database your login can open.

For production I'd still use a role with only SELECT grants. The app is the second lock, not the first.

Not there yet: SSH tunnels (so no bastion hosts yet), signed builds, query history, export.

Two questions for this sub:
1. Can you think of a statement that passes the check and does harm inside a read-only transaction?
2. What would you want it to read beyond tables, columns and foreign keys (comments, enums, views)?

Source: https://github.com/dithzz/sayql
Postgres guide: https://sayql.dev/docs/postgresql-plain-english/
