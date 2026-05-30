# Release- & Update-Management

Maintainer-Runbook: wie eine Version geschnitten, gebaut, signiert,
veröffentlicht und an Nutzer ausgeliefert wird.

## Überblick

```
  pnpm release minor          # bumpt Version, committet, taggt vX.Y.Z
        │
        ▼  git push --follow-tags
  ┌──────────────────────────────────────────────────────────────┐
  │ .github/workflows/release.yml  (Trigger: Tag v*)              │
  │                                                               │
  │  1. git-cliff (cliff.toml)  → Release-Notes aus den Commits   │
  │  2. tauri-action (ubuntu-24.04):                              │
  │       • baut deb / rpm / AppImage                             │
  │       • signiert die Updater-Artefakte (minisign)            │
  │       • erstellt ein GitHub-Release als DRAFT                 │
  │       • lädt Assets + latest.json hoch                        │
  └──────────────────────────────────────────────────────────────┘
        │
        ▼  Du prüfst das Draft-Release und klickst „Publish"
   → /releases/latest/download/latest.json wird live
   → der In-App-Updater bietet die neue Version an
```

> **Windows ist derzeit nicht im Release** (ggml-Symbolkollision beim
> Linken von whisper.cpp + llama.cpp, MSVC). Tracking:
> [#1](https://github.com/ks98/voicetypex/issues/1). Sobald gelöst, wird
> `windows-latest` in `release.yml` wieder aktiviert.

## Versionierung — eine Quelle

`src-tauri/Cargo.toml` (`[package] version`) ist die **Single Source of
Truth**. Die App zeigt diese Version (`env!("CARGO_PKG_VERSION")`), und
`tauri.conf.json` hat **kein** `version`-Feld (es erbt aus Cargo.toml).

`scripts/bump-version.mjs` (via `pnpm release`) hält die zwei weiteren
Stellen synchron, die Cargo nicht selbst erbt: `package.json` und den
`voicetypex`-Eintrag in `Cargo.lock`.

```bash
pnpm release patch          # 0.1.0 -> 0.1.1
pnpm release minor          # 0.1.0 -> 0.2.0
pnpm release major          # 0.1.0 -> 1.0.0
pnpm release 1.0.0-rc.1      # explizite Version (auch Pre-Release)
pnpm release patch --dry-run # nur anzeigen, nichts schreiben
pnpm release patch --no-git  # Dateien bumpen, ohne commit/tag
```

Default (ohne `--no-git`): committet `chore(release): vX.Y.Z` und setzt
das annotierte Tag `vX.Y.Z`. Der **Push bleibt ein bewusster manueller
Schritt** (`git push --follow-tags`).

**Erstes Release** (Version unverändert, z. B. 0.1.0): das Script
verweigert eine identische Version — Tag von Hand setzen:

```bash
git tag -a v0.1.0 -m "v0.1.0" && git push --follow-tags
```

## Changelog

`cliff.toml` (git-cliff) erzeugt die Release-Notes aus
[Conventional Commits](https://www.conventionalcommits.org/). Gruppen:
Features (`feat`), Bug Fixes (`fix`), Performance (`perf`), Tuning
(`tune` — projekteigen), Refactor, Documentation, Diagnostics (`diag`),
Dependencies (`chore(deps)`). Rausch-Typen (`chore`, `test`, `ci`,
`build`, `chore(release)`) erscheinen **nicht** im Changelog.

> **Neuer Commit-Type?** In `cliff.toml` unter `commit_parsers`
> ergänzen — sonst verschwindet er im Catch-all.

Lokale Vorschau: `git cliff --unreleased` bzw. `git cliff --latest`.

## CI-Struktur

- **`ci.yml`** — bei jedem Push auf `main` und jedem PR: `cargo fmt`,
  `clippy`, `pnpm lint`/`format:check`, `cargo test`, `vitest`,
  `cargo audit`/`pnpm audit`. Linux (vollständig) + Windows
  (`cargo check`-Smoke-Test) + Audit.
- **`release.yml`** — nur auf Tags `v*`: Changelog + tauri-action.

## Auto-Update

Self-Update läuft **pro Kanal** — es gibt nicht „einen" Updater:

| Paket | Update-Weg |
|---|---|
| **AppImage** (Linux) | In-App-Updater (*Einstellungen → Diagnose → Updates*) |
| **NSIS** (Windows, sobald reaktiviert) | In-App-Updater |
| **`.deb`** | Paketmanager bzw. Re-Download vom GitHub-Release |
| **`.rpm`** | Paketmanager bzw. Re-Download vom GitHub-Release |

Der In-App-Updater (`tauri-plugin-updater` + `tauri-plugin-process`)
prüft den **Endpoint**:

```
https://github.com/ks98/voicetypex/releases/latest/download/latest.json
```

`/releases/latest/` zeigt immer auf das neueste **veröffentlichte**
(nicht-Draft, nicht-Prerelease) Release — deshalb wirkt der Draft-Gate:
solange ein Release Draft ist, sieht der Updater es nicht. Der Download
ist **klick-gated** (volles Bundle, keine Deltas) und vor der
Installation **minisign-verifiziert**.

## Signing-Key (kritisch)

Der Updater verifiziert jedes Update gegen eine **minisign-/Ed25519**-
Signatur — unabhängig von OS-Code-Signierung (Authenticode/SmartScreen).

- **Private Key:** `~/.tauri/voicetypex-updater.key` (chmod 600,
  passwortlos). **Niemals committen.** Inhalt liegt als GitHub-Secret
  `TAURI_SIGNING_PRIVATE_KEY` (Settings → Secrets → Actions).
- **Public Key:** in `tauri.conf.json` unter `plugins.updater.pubkey`
  eingebettet (nicht geheim).
- **Rotation:** nur **vor** dem ersten veröffentlichten Release gefahrlos
  möglich. Danach würde ein neuer Key die Updates für **bereits
  installierte** Nutzer brechen (sie verifizieren gegen den alten
  Public Key). Den Private Key sicher aufbewahren.

Neuen Key erzeugen (falls nötig, vor dem ersten Release):
`pnpm tauri signer generate -w ~/.tauri/voicetypex-updater.key`, dann
Pubkey in `tauri.conf.json` und Private Key ins GitHub-Secret.

## Pre-Releases

`-rc.N` / `-beta.N`-Tags werden in `release.yml` als GitHub-Prerelease
markiert und damit von `/releases/latest/` **übersprungen** — stabile
Nutzer bekommen sie nicht angeboten.

## Plattform-Status

- **Linux** (deb / rpm / AppImage): vollständig im Release, mit
  Auto-Updater (AppImage).
- **Windows** (NSIS): **zurückgestellt** — siehe
  [#1](https://github.com/ks98/voicetypex/issues/1). Der gesamte
  Vulkan-Build kompiliert bereits; nur das Linken der zwei ggml-Kopien
  (whisper.cpp + llama.cpp) bricht auf MSVC. `cargo check` läuft in CI
  als Smoke-Test weiter.
- **macOS**: out of scope.

## Erst-Setup-Checkliste (einmalig)

1. `gh auth login` (Scopes `repo`, `workflow`).
2. GitHub-Secret `TAURI_SIGNING_PRIVATE_KEY` setzen:
   `gh secret set TAURI_SIGNING_PRIVATE_KEY --repo ks98/voicetypex < ~/.tauri/voicetypex-updater.key`
3. Optional: `v*` als *Protected Tag* (Settings → Tags), damit nur
   Maintainer Releases schneiden.
