# Releasing AskDB

This guide covers building AskDB installers, signing them so macOS and Windows
trust them, and shipping updates through the built-in updater.

Nothing secret is committed to this repository. Every key, certificate and
password below lives on your machine or in GitHub Actions secrets. Where the
repo has a placeholder, this guide says what replaces it.

| What | Where it lives | Placeholder to replace |
| --- | --- | --- |
| Updater public key | `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` | `REPLACE_WITH_UPDATER_PUBLIC_KEY` |
| Update feed URL | `src-tauri/tauri.conf.json` → `plugins.updater.endpoints` | `OWNER/REPO` |
| Updater private key | GitHub secret `TAURI_SIGNING_PRIVATE_KEY` | — |
| Apple Developer ID certificate | GitHub secrets `APPLE_CERTIFICATE*` | — |
| Notarization API key | GitHub secrets `APPLE_API_*` | — |
| Windows signing | `src-tauri/tauri.conf.json` → `bundle.windows` | see [Windows](#4-windows-code-signing) |

## 1. Build locally (unsigned)

Prerequisites: Rust (stable), Node.js LTS, pnpm 10, and Xcode Command Line
Tools on macOS (`xcode-select --install`).

```bash
pnpm install
pnpm tauri build
```

Output, for the machine's own architecture:

- `src-tauri/target/release/bundle/macos/AskDB.app`
- `src-tauri/target/release/bundle/dmg/AskDB_<version>_<arch>.dmg`

Other targets:

```bash
rustup target add x86_64-apple-darwin aarch64-apple-darwin
pnpm tauri build --target universal-apple-darwin   # one .app for Intel + Apple silicon
pnpm tauri build --bundles dmg                     # only the .dmg
pnpm tauri build --debug                           # faster build, devtools enabled
```

An unsigned build runs on the machine that built it. Anyone who downloads it
gets a Gatekeeper warning ("Apple could not verify AskDB is free of
malware…") and has to approve it in System Settings → Privacy & Security.
That is fine for testing; paying customers need the signed and notarized
build from the next section.

### Version numbers

The version shown to users and compared by the updater comes from
`src-tauri/tauri.conf.json` → `version`. Keep `package.json` and
`src-tauri/Cargo.toml` in step with it. Bump all three, commit, then tag
`vX.Y.Z` (see [section 5](#5-publishing-a-release)).

## 2. Updater signing key

The updater refuses any update that isn't signed with your updater key. This
key is separate from Apple and Windows code signing, and it is free.

1. Generate the key pair. Keep the private key out of the repo:

   ```bash
   pnpm tauri signer generate -w ~/.tauri/askdb-updater.key
   ```

   Pick a password when asked. This writes `~/.tauri/askdb-updater.key`
   (private) and `~/.tauri/askdb-updater.key.pub` (public).

2. Put the **public** key in `src-tauri/tauri.conf.json`, replacing
   `REPLACE_WITH_UPDATER_PUBLIC_KEY` with the full contents of
   `askdb-updater.key.pub`. Commit this; it is meant to be public.

3. Add two GitHub Actions secrets:
   - `TAURI_SIGNING_PRIVATE_KEY`: the contents of `askdb-updater.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password from step 1

4. Back the private key up in a password manager. **If you lose it, installed
   copies can never auto-update again.** Users would have to download a new
   version by hand.

5. Replace `OWNER/REPO` in `plugins.updater.endpoints` with the GitHub
   repository that hosts releases, for example
   `https://github.com/acme/askdb/releases/latest/download/latest.json`.

To build signed updater artifacts locally (usually CI does this):

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/askdb-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD='…'
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":true}}'
```

`bundle.createUpdaterArtifacts` is `false` in the committed config so that
plain `pnpm tauri build` works without the private key. The release workflow
switches it on. Until the placeholder public key is replaced, a build with
updater artifacts fails with `failed to decode pubkey`, which is intended:
it stops a release that installed copies could never verify.

### Where updates are hosted

The release workflow uploads `latest.json` and the signed update archives
(`AskDB.app.tar.gz` and `.sig` on macOS) to the GitHub Release. Installed apps
fetch `releases/latest/download/latest.json`, which only resolves to a release
that is **published** (not a draft) and **not marked pre-release**.

GitHub Releases on a **private** repository are not downloadable by your
users. Either make the release repository public (the source repo can stay
private: publish to a separate public "askdb-releases" repo with
tauri-action's `owner`/`repo` inputs), or host `latest.json` and the archives
on your own server or CDN and point `endpoints` there. Public downloads also
mean anyone can download the app, so licensing (trial, license keys) has to be
enforced inside the app, not by hiding the download.

### Checking for updates in the app

The updater plugin is registered in `src-tauri/src/lib.rs` and allowed by the
`updater:default` permission, but nothing calls it yet. To add an
"update available" prompt, install the JS bindings
(`pnpm add @tauri-apps/plugin-updater @tauri-apps/plugin-process`, both MIT/Apache-2.0)
and follow <https://v2.tauri.app/plugin/updater/#checking-for-updates>.
For a restart button, also register `tauri-plugin-process`.

## 3. macOS: Developer ID signing and notarization

Without both, macOS shows users a warning that the app may be unsafe, and on
recent macOS versions they have to dig into System Settings to open it at all.
Signed and notarized apps open with only the standard "downloaded from the
internet" prompt.

### What you need

1. **Apple Developer Program membership** (USD 99/year) at
   <https://developer.apple.com/programs/>. Enrolling as an organization shows
   your company name in the install prompt and needs a D-U-N-S number;
   enrolling as an individual shows your personal name.
2. A **Developer ID Application** certificate (not "Apple Development" or
   "Mac App Store"). Only the Account Holder can create it.
3. An **App Store Connect API key** for notarization (preferred over Apple ID
   + app-specific password because it isn't tied to one person's login).

### Create the certificate

1. On your Mac, open Keychain Access → Certificate Assistant → Request a
   Certificate From a Certificate Authority. Save the `.certSigningRequest`
   to disk.
2. At <https://developer.apple.com/account/resources/certificates/add>,
   choose **Developer ID Application**, upload the request, and download the
   `.cer`. Double-click it to add it to your login keychain.
3. Find the signing identity name:

   ```bash
   security find-identity -v -p codesigning
   # "Developer ID Application: Your Company (TEAMID1234)"
   ```

4. For CI, export the certificate and its private key from Keychain Access as
   a `.p12` with a password, then base64 it:

   ```bash
   base64 -i DeveloperID.p12 | pbcopy
   ```

### Create the notarization API key

1. App Store Connect → Users and Access → Integrations → App Store Connect
   API → Team Keys → generate a key with the **Developer** role.
2. Download `AuthKey_<KEYID>.p8` (Apple lets you download it only once). Note
   the **Key ID** and the **Issuer ID** shown on that page.

### GitHub secrets for macOS

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | base64 of the `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | password set when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Company (TEAMID1234)` |
| `KEYCHAIN_PASSWORD` | any random string; used for a temporary keychain on the runner |
| `APPLE_API_ISSUER` | Issuer ID |
| `APPLE_API_KEY` | Key ID |
| `APPLE_API_KEY_P8` | full text of `AuthKey_<KEYID>.p8` |

With these set, `pnpm tauri build` signs the app with the hardened runtime
(`bundle.macOS.hardenedRuntime` is on), submits it to Apple's notary service,
waits for approval, and staples the ticket to the `.app` before building the
`.dmg`. Notarization usually takes a few minutes; the first submission for a
new team can take longer.

### Signing and notarizing on your own Mac

With the certificate in your login keychain:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Company (TEAMID1234)"
export APPLE_API_ISSUER="…"
export APPLE_API_KEY="…"
export APPLE_API_KEY_PATH="$HOME/.private_keys/AuthKey_XXXXXXXXXX.p8"
pnpm tauri build --target universal-apple-darwin
```

You can also set `bundle.macOS.signingIdentity` in `tauri.conf.json` instead
of `APPLE_SIGNING_IDENTITY`; the identity name isn't secret, but keeping it in
an env var lets unsigned local builds keep working.

Alternative to the API key: `APPLE_ID`, `APPLE_PASSWORD` (an app-specific
password from <https://account.apple.com>) and `APPLE_TEAM_ID`.

### Verify a build

```bash
codesign --verify --deep --strict --verbose=2 src-tauri/target/release/bundle/macos/AskDB.app
spctl --assess --type execute --verbose src-tauri/target/release/bundle/macos/AskDB.app
# expect: "accepted  source=Notarized Developer ID"
xcrun stapler validate src-tauri/target/release/bundle/macos/AskDB.app
```

Tauri notarizes and staples the `.app`, which is what Gatekeeper checks when
the user opens it. To also notarize and staple the `.dmg` itself (so the
disk image passes checks offline before it's mounted):

```bash
xcrun notarytool submit AskDB_0.1.0_aarch64.dmg --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait
xcrun stapler staple AskDB_0.1.0_aarch64.dmg
```

## 4. Windows code signing

Unsigned Windows installers trigger Microsoft Defender SmartScreen ("Windows
protected your PC"). Since June 2023, publicly trusted code-signing keys must
live on a hardware security module, so you can no longer just buy a `.pfx`
file. The realistic options:

| Option | Rough cost | Notes |
| --- | --- | --- |
| **Azure Artifact Signing** (formerly Azure Trusted Signing) | ~USD 10/month | Cheapest and simplest. Microsoft verifies your identity; certificates are managed for you. Check current eligibility on Microsoft's site; it has been limited by country and, for individuals, by region. |
| **Cloud HSM signing from a CA** (DigiCert KeyLocker, SSL.com eSigner, Sectigo, …) | ~USD 200–600/year | OV certificate held in the CA's cloud; signed through their CLI. |
| **OV/EV certificate on a USB token** | ~USD 200–600/year | Works for local builds; awkward in CI because the token must be physically attached. |

EV certificates no longer skip SmartScreen's reputation check, so OV is
usually enough. New certificates build reputation as more users install the
app; early users may still see a warning for a while.

### Wiring it into the build

All options except the USB token plug in through `bundle.windows.signCommand`,
which Tauri runs for each file it needs to sign (`%1` is the file path). For
Azure Artifact Signing with
[`trusted-signing-cli`](https://github.com/Levminer/trusted-signing-cli):

```json
"windows": {
  "signCommand": "trusted-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <certificate-profile> -d AskDB %1"
}
```

It reads `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` and `AZURE_TENANT_ID` from
the environment; add those as GitHub secrets and pass them to the
tauri-action step. For a CA's cloud HSM, use their signing tool in
`signCommand` the same way. For a certificate installed in the Windows
certificate store, set `certificateThumbprint` instead, plus `timestampUrl`
pointing at your CA's timestamp server (for example
`http://timestamp.digicert.com`).

Then uncomment the `windows-latest` entry in
`.github/workflows/release.yml`. The Windows build produces an NSIS `-setup.exe`
and an `.msi`; the updater installs updates in `passive` mode (progress bar,
no prompts).

## 5. Publishing a release

With the secrets in place:

1. Bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and
   `package.json`. Commit.
2. Tag and push:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. `.github/workflows/release.yml` builds Apple silicon and Intel macOS
   bundles (and Windows once enabled), signs and notarizes them, and uploads
   everything plus `latest.json` to a **draft** GitHub Release.
4. Download the draft's `.dmg`, install it on a clean Mac, and check it opens
   without a security warning.
5. Publish the draft. Installed copies see the update the next time they
   check.

## 6. Rebranding

"AskDB" is a placeholder name. To rename the app:

- `src-tauri/tauri.conf.json`: `productName`, `app.windows[0].title`,
  `bundle.publisher`, `bundle.copyright`, `bundle.shortDescription`,
  `bundle.longDescription`
- `.github/workflows/release.yml`: `releaseName`
- Icon: replace `src-tauri/icons/app-icon.svg` (1024×1024, artwork inside the
  centered 824×824 area so it matches other macOS icons), then run
  `pnpm tauri icon src-tauri/icons/app-icon.svg`. The command also creates
  `icons/android` and `icons/ios`; delete them, since this is a desktop app.
- `identifier` (`com.askdb.app`): change it **before the first public
  release** if at all. It ties together the macOS keychain entries, app data
  folder and update channel; changing it later makes installed copies treat
  the new app as a different one.

## Shipping the MCP server (`askdb-mcp`)

The Settings dialog points Claude at `askdb-mcp` next to the app's executable. To ship it, build it for each target
and add it as a Tauri sidecar: copy `target/<triple>/release/askdb-mcp` to `src-tauri/binaries/askdb-mcp-<triple>`
(e.g. `askdb-mcp-aarch64-apple-darwin`) and add `"externalBin": ["binaries/askdb-mcp"]` under `bundle` in
`tauri.conf.json`. Tauri strips the triple and places it in `AskDB.app/Contents/MacOS/`. Sign it like the app.
