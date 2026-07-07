# Scav Mod Manager

> **This entire project — every line of code, every commit, and this README — was written by
> an AI (Claude), not a human developer.** No hand-written code has been added or reviewed by a
> professional engineer. Use it accordingly: read the source before trusting it with your files,
> and treat it as an experiment rather than a polished, vendor-supported product.

An **r2modman-style mod manager** for **Casualties: Unknown / Scav Prototype**, built with
Tauri v2 (Rust) + React. Works on **Linux and Windows**, installs mods from
[Nexus Mods](https://www.nexusmods.com/scavprototype) (the full ~130-mod catalog), and manages
the **BepInEx** mod loader.

## Download

Grab the latest build for your platform from the
[Releases page](https://github.com/VeryCoolMike/scav-mod-manager/releases/latest):

- **Windows:** the `.msi` installer, or the standalone `_portable.exe` (no install needed)
- **Linux:** the `.AppImage` (portable), `.deb` (Debian/Ubuntu), or `.rpm` (Fedora/RHEL)

The app checks for updates on launch and can update itself in place (see below) — after the
first install you generally won't need to come back here.

## Auto-updates

Every release is code-signed and published with an update manifest. On startup, the app checks
GitHub Releases for a newer version; if one is available, a banner lets you update and restart
with one click — no manual downloading required after the first install.

## Features

- 🔍 **Auto-detects** the game (Steam AppID `4576490`, itch, or manual folder pick)
- ⬇️ **One-click BepInEx** install/repair/uninstall (BepInEx 5, x64, Mono)
- 🔗 **One-click Nexus login (SSO)** — no key copying. Click *Login with Nexus*, hit **Authorize**
  once in the browser, and the app captures your key automatically (the Vortex/MO2 flow)
- 🌐 **Browse Nexus** — trending / newest / recently-updated feeds, plus paste-a-URL/nxm install
- 📥 **`nxm://` handler** — click *“Mod Manager Download”* on the Nexus site and the mod installs
  straight into the active profile
- 🧩 **Enable/disable** mods without deleting them; loader files are protected from being clobbered
- 🗂️ **Profiles** — create, clone, switch, delete; **export** as a portable zip bundle or a
  lightweight mod-list code, and **import** bundles
- 🔄 **Update checks** with per-mod badges · ♥ **Endorse** from inside the app
- ▶️ **Launch modded or vanilla** (vanilla toggles the BepInEx doorstop off for that run)

## Signing in (built into the app)

Mods live on **Nexus**, which requires a (free) account to download — this is Nexus's rule and
applies to every downloader. The app makes it as painless as possible with **in-app SSO**:

1. Click **Login with Nexus**.
2. A browser tab opens; click **Authorize** once.
3. The app receives your API key over a websocket (`wss://sso.nexusmods.com`) — no copying.

After that:

- **Free account:** browse in-app, then click *“Mod Manager Download”* on a mod's Nexus page.
  Your browser hands the app an `nxm://` link and it installs automatically.
- **Premium account:** the **Install** button downloads directly in-app (auto-detected).

> Nexus's API has no keyword-search endpoint, so in-app discovery uses the trending/newest/updated
> feeds and direct URL/ID lookup; full-text search happens on the website and flows back through
> the `nxm://` button.

A manual API-key field is available under **Settings → Advanced** as a fallback.

<details>
<summary>Why not a fully account-free source?</summary>

GameBanana (game `24260`) has a public, no-login API, and the app retains a backend client for
it — but it only hosts **5** mods for this game, versus ~130 on Nexus. So Nexus is the default.
There is no legitimate way to download from Nexus with no account at all.
</details>

## Linux / Proton note

Casualties: Unknown is a Windows build; on Linux it usually runs through **Steam Proton**.
BepInEx is installed as the Windows x64 build and needs a one-time Steam launch option
(the app shows it during setup):

```
WINEDLLOVERRIDES="winhttp=n,b" %command%
```

Set it in Steam → right-click the game → Properties → Launch Options.

## Development

```bash
pnpm install
pnpm tauri dev      # run the app with hot reload
pnpm tauri build    # produce installers
cargo test --manifest-path src-tauri/Cargo.toml   # backend unit tests
```

Requirements: Node 18+, Rust stable, and the
[Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/)
(on Linux: `webkit2gtk-4.1`, `gtk3`, etc.).

## Packaging

`pnpm tauri build` produces, per platform:

- **Windows:** `.msi` — **use the MSI**, not NSIS; the `nxm://` deep-link handler does not
  register correctly with the NSIS bundler (Tauri issue #10095). CI also publishes a standalone
  portable `.exe` (just the raw binary, no installer) alongside it.
- **Linux:** `.AppImage`, `.deb`, and `.rpm`. The `nxm://` scheme is registered via `xdg-mime`
  (verify with `xdg-mime query default x-scheme-handler/nxm`).

## Architecture

```
src/                      React UI (Setup, Installed, Online, Profiles, Settings)
  lib/{api,types,ui}.ts   invoke() wrappers, types, toast/primitives
src-tauri/src/
  lib.rs        Tauri builder, single-instance + deep-link (nxm) wiring
  commands.rs   #[tauri::command] surface
  state.rs      Settings, app-data paths, shared AppState
  game.rs       Steam/itch/manual detection + launch
  bepinex.rs    BepInEx download/install/repair/uninstall + doorstop toggle
  nexus.rs      Nexus API v1 client (primary source)
  nexus_sso.rs  One-click SSO login (websocket → API key, no manual entry)
  nxm.rs        nxm:// URL parsing + install driver
  gamebanana.rs GameBanana API client (secondary, account-free; only 5 mods)
  mods.rs       archive extract, tree normalization, enable/disable, game sync
  profiles.rs   profile CRUD + import/export
  updates.rs    update checks vs Nexus files
```

Mods are stored per-profile under the app-data dir and **synced into the game's
`BepInEx/plugins/`** on modded launch; a manifest tracks synced files so switching profiles
cleans up the previous set. The loader itself (`winhttp.dll`, `BepInEx/core/**`) is never
overwritten by a mod.

## Data locations

- Linux: `~/.local/share/scav-mod-manager/`
- Windows: `%APPDATA%\scav-mod-manager\`

Contains `settings.json` and `profiles/<name>/` (each with `profile.json` and the mod files).
