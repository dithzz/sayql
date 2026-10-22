# Patched driver crates

Two database drivers are used from patched copies here, through `[patch.crates-io]` in
`src-tauri/Cargo.toml`. Each is the published crate with the small changes below, every one marked
with a `SayQL patch` comment. The tests in `src-tauri/src/db/integration_tests.rs` cover each change
against real servers.

To update one: copy the new release from `~/.cargo/registry/src/*/<crate>-<version>`, reapply the
changes (search the old copy for `SayQL patch`), bump the version in `Cargo.toml`, and run
`scripts/test-databases.sh`. Drop a patch once the upstream crate has the fix.

## sqlx-mysql 0.9.0

- **MariaDB `ed25519` sign-in** (`client_ed25519`), which sqlx doesn't implement:
  `src/protocol/auth.rs`, `src/protocol/connect/auth_switch.rs` (32-byte nonce),
  `src/connection/auth.rs` (`sign_ed25519`). The signature is Ed25519 over the server's nonce, with
  SHA-512 of the password as the expanded secret key, as in MariaDB's `crypto_sign`. This adds
  `ed25519-dalek` and `sha2` 0.10 to the crate's dependencies.
- **MariaDB JSON columns**, which MariaDB stores as LONGTEXT and marks only in its extended type
  info: the client asks for `MARIADB_CLIENT_EXTENDED_TYPE_INFO` (`src/connection/stream.rs`). Columns
  whose format is `json` are read as JSON (`src/protocol/text/column.rs`). MySQL servers don't
  offer the capability, so nothing changes for them.

Test files, examples and the crate's own lockfile are left out.

## tiberius 0.13.0

- **Exact `money` and `smallmoney`:** decoded as a 4-place `Numeric`, not an `f64`, which loses
  digits above about 900 billion (`src/tds/codec/column_data/money.rs`).

The crate's tests, examples, docs, Docker and Nix files are left out.

Both crates are licensed under MIT or Apache-2.0; their license files are in their folders.
