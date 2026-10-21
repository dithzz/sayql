# Contributing to SayQL

Thanks for helping make SayQL better. Bug reports, ideas, docs fixes and code are all welcome.

## Before you start

- **Bugs and ideas:** open an [issue](https://github.com/dithzz/sayql/issues/new/choose). For questions and
  general feedback, use [Discussions](https://github.com/dithzz/sayql-releases/discussions).
- **Security problems:** don't open a public issue. See [SECURITY.md](SECURITY.md).
- **Larger changes:** open an issue first so we can agree on the approach before you spend time on it.

## Set up

You need:

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) LTS and [pnpm](https://pnpm.io/) 10
- The Tauri system dependencies for your platform
  ([guide](https://v2.tauri.app/start/prerequisites/)):
  - **macOS:** Xcode Command Line Tools (`xcode-select --install`)
  - **Windows:** Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11)
  - **Linux (Debian/Ubuntu):** `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libdbus-1-dev`

Then:

```bash
pnpm install
pnpm tauri dev      # run the desktop app
pnpm test           # frontend and backend tests
```

To work on the UI only, run `pnpm dev` and open http://localhost:1420. A mock backend serves demo data,
so no database or AI key is needed.

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains the folder structure and code conventions. The
README covers the optional integration tests against PostgreSQL, MySQL and SQL Server.

## Making a change

1. Fork the repository and create a branch from `main`.
2. Keep the change focused: one fix or feature per pull request.
3. Match the surrounding code: feature folders in `src/features`, design tokens instead of raw colours,
   thin Tauri commands, tests next to the code they cover.
4. Add or update tests, and run `pnpm test` and `pnpm build` (type check) before you push.
5. Open a pull request describing what changed and why. CI runs the same checks.

## Ground rules for SayQL features

- **Never weaken read-only safety.** Anything that could change data must keep going through the write
  confirmation, and the MCP server must stay read-only.
- **Never send table rows to an AI provider** from the Ask box. Only questions, schema and SQL.
- **Secrets stay in the OS keychain**, never in files or logs.

## Docs and website

The website is `site/`. Docs pages are in `docs-site/pages/` and are built with
`python3 scripts/build_docs.py`.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE),
the same license as the project.

Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
