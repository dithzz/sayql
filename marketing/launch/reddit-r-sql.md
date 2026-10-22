# Reddit: r/SQL

## Rules check (do this before posting)

I couldn't read r/SQL's rules automatically: Reddit blocks automated access, and no copy of the rules turned up in search. Before posting:

1. Open https://www.reddit.com/r/SQL/about/rules and read every rule.
2. Look for: a self-promotion limit, a required flair (pick the one for tools/discussion if there is one), and a weekly or pinned thread for tools.
3. If tool posts must go in a pinned thread, post the body below there instead.

General Reddit norms that apply regardless:
- Say plainly that you made it.
- Post once. Don't cross-post the same text to several subs on the same day.
- Reply to comments; don't post and leave.

## Title

```
I made a free, open-source app that writes SQL from plain English, and runs reads in a read-only transaction
```

## Body

```
I'm the maker, so take this with that in mind.

SayQL is a desktop app (macOS, Windows, Linux). You type what you want, it writes SQL against your actual schema, and you see the query before anything runs. It's Apache-2.0 and free.

What r/SQL might care about:
- You always see the SQL. It's an editor, not a black box. Follow-ups like "only 2025" edit the query you have.
- Read-only by default. A statement only counts as a read if it starts with a read keyword and has no write keyword outside strings and comments. Reads then run inside a read-only transaction, so the database rejects writes even if the check misses one. Anything that could write shows you the statement and asks.
- Running a selection never cuts a statement in half, so a DELETE can't run without its WHERE.
- The AI gets the schema, never table rows. Bring your own key, or use Ollama for fully local.
- PostgreSQL, MySQL/MariaDB, SQLite, SQL Server, Azure SQL.

Not there yet: SSH tunnels, signed builds, query history, export.

I'd like to hear where it writes bad SQL, and whether you can get a write past the read-only check.

Source: https://github.com/dithzz/sayql
Site: https://sayql.dev
```
