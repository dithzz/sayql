# Reddit: r/selfhosted

## Rules check (do this before posting)

Reddit blocks automated access, so I read the rules through mod announcements and search results, not the rules page itself. What they say:

- **Projects younger than 3 months go in the weekly "New Project" megathread.** Standalone posts about new projects are removed and the author is pointed to the megathread. SayQL's first release was 2026-10-21, so **post this as a comment in the current megathread, not as its own post.** The megathread is pinned; a new one goes up each Friday, but you can comment any day.
- The megathread suggests a template: project name, then repo or website link, then a short description.
- No shills or unsolicited advertising; low-effort self-promotion is removed.
- In March 2026 the mods said they no longer police whether AI was used to build a project.

Confirm at https://www.reddit.com/r/selfhosted/about/rules before posting; rules change.

Fit: SayQL is a desktop app, not a server you host. The angle for this sub is that it has no server or account, and with Ollama or LM Studio nothing leaves your network. Lead with that. You can make a standalone post after 2027-01-21 (3 months), if the rules still say so.

## Megathread comment

```
**SayQL**: plain English to SQL, fully local if you want it

Repo: https://github.com/dithzz/sayql
Site: https://sayql.dev

I made this. It's a desktop app (macOS, Windows, Linux) that turns plain English into SQL for the databases you already run: PostgreSQL, MySQL/MariaDB, SQLite, SQL Server.

Why it might fit here:
- No account, no server of mine, no telemetry. The only call home is an update check to GitHub.
- Point it at Ollama or LM Studio and no question or schema leaves your machine. Or use any OpenAI-compatible endpoint (vLLM, LiteLLM).
- The AI sees your schema, never your rows.
- Read-only by default: reads run in a read-only transaction the database enforces, and anything that could write asks first. Handy on your homelab DBs.
- Apache-2.0.

Not there yet: SSH tunnels, signed builds, query history, export.

Local-model setup guide: https://sayql.dev/docs/local-ai-sql-ollama/

Feedback welcome, especially on how well small local models do.
```
