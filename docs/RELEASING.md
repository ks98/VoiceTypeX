# Release & Update Management

Maintainer runbook: how a version gets cut, built, signed, published, and
delivered to users.

## Overview

```
  pnpm release minor          # bumps version, commits, tags vX.Y.Z
        │
        ▼  git push --follow-tags
  ┌──────────────────────────────────────────────────────────────┐
  │ .github/workflows/release.yml  (trigger: tag v*)              │
  │                                                               │
  │  1. git-cliff (cliff.toml)  → release notes from the commits  │
  │  2. tauri-action (ubuntu-24.04):                              │
  │       • builds deb / rpm / AppImage                           │
  │       • signs the updater artifacts (minisign)                │
  │       • creates a GitHub release as a DRAFT                   │
  │       • uploads assets + latest.json                          │
  └──────────────────────────────────────────────────────────────┘
        │
        ▼  You review the draft release and click "Publish"
   → /releases/latest/download/latest.json goes live
   → the in-app updater offers the new version
```

> **Windows is back in the release** (NSIS installer with STT + Vulkan +
> Cloud/Ollama LLM). The embedded llama-cpp-2 has been removed on Windows
> ([#1](https://github.com/ks98/VoiceTypeX/issues/1): ggml symbol collision
> when MSVC links whisper.cpp + llama.cpp) — a local LLM runs there
> through a self-installed Ollama daemon or via the cloud.

## Versioning — a single source

`src-tauri/Cargo.toml` (`[package] version`) is the **single source of
truth**. The app displays this version (`env!("CARGO_PKG_VERSION")`), and
`tauri.conf.json` has **no** `version` field (it inherits from Cargo.toml).

`scripts/bump-version.mjs` (via `pnpm release`) keeps in sync the two
remaining spots that Cargo does not inherit on its own: `package.json` and
the `voicetypex` entry in `Cargo.lock`.

```bash
pnpm release patch          # 0.1.0 -> 0.1.1
pnpm release minor          # 0.1.0 -> 0.2.0
pnpm release major          # 0.1.0 -> 1.0.0
pnpm release 1.0.0-rc.1      # explicit version (also pre-release)
pnpm release patch --dry-run # only show, write nothing
pnpm release patch --no-git  # bump files, without commit/tag
```

Default (without `--no-git`): commits `chore(release): vX.Y.Z` and creates
the annotated tag `vX.Y.Z`. The **push remains a deliberate manual step**
(`git push --follow-tags`).

**First release** (version unchanged, e.g. 0.1.0): the script refuses an
identical version — set the tag by hand:

```bash
git tag -a v0.1.0 -m "v0.1.0" && git push --follow-tags
```

## Changelog

`cliff.toml` (git-cliff) generates the release notes from
[Conventional Commits](https://www.conventionalcommits.org/). Groups:
Features (`feat`), Bug Fixes (`fix`), Performance (`perf`), Tuning
(`tune` — project-specific), Refactor, Documentation, Diagnostics (`diag`),
Dependencies (`chore(deps)`). Noise types (`chore`, `test`, `ci`,
`build`, `chore(release)`) do **not** appear in the changelog.

> **A new commit type?** Add it in `cliff.toml` under `commit_parsers` —
> otherwise it disappears into the catch-all.

Local preview: `git cliff --unreleased` or `git cliff --latest`.

## CI structure

- **`ci.yml`** — on every push to `main` and every PR: `cargo fmt`,
  `clippy`, `pnpm lint`/`format:check`, `cargo test`, `vitest`,
  `cargo audit`/`pnpm audit`. Linux (full) + Windows
  (`cargo check` smoke test) + audit.
- **`release.yml`** — only on `v*` tags: changelog + tauri-action.

## Auto-update

Self-update runs **per channel** — there is no single "one" updater:

| Package | Update path |
|---|---|
| **AppImage** (Linux) | In-app updater (*Settings → Diagnostics & tests → Updates*) |
| **NSIS** (Windows, once reactivated) | In-app updater |
| **`.deb`** | Package manager or re-download from the GitHub release |
| **`.rpm`** | Package manager or re-download from the GitHub release |

The in-app updater (`tauri-plugin-updater` + `tauri-plugin-process`)
queries the **endpoint**:

```
https://github.com/ks98/VoiceTypeX/releases/latest/download/latest.json
```

`/releases/latest/` always points to the newest **published**
(non-draft, non-prerelease) release — which is why the draft gate works:
as long as a release is a draft, the updater does not see it. The download
is **click-gated** (full bundle, no deltas) and **minisign-verified**
before installation.

## Signing key (critical)

The updater verifies every update against a **minisign/Ed25519**
signature — independent of OS code signing (Authenticode/SmartScreen).

- **Private key:** `~/.tauri/voicetypex-updater.key` (chmod 600,
  passwordless). **Never commit it.** Its contents live as the GitHub
  secret `TAURI_SIGNING_PRIVATE_KEY` (Settings → Secrets → Actions).
- **Public key:** embedded in `tauri.conf.json` under
  `plugins.updater.pubkey` (not secret).
- **Rotation:** only safe **before** the first published release.
  Afterwards, a new key would break updates for **already installed**
  users (they verify against the old public key). Keep the private key in
  a safe place.

Generate a new key (if needed, before the first release):
`pnpm tauri signer generate -w ~/.tauri/voicetypex-updater.key`, then put
the pubkey in `tauri.conf.json` and the private key in the GitHub secret.

## Pre-releases

`-rc.N` / `-beta.N` tags are marked as a GitHub prerelease in `release.yml`
and are thus **skipped** by `/releases/latest/` — stable users are not
offered them.

## Platform status

- **Linux** (deb / rpm / AppImage): in the release. The AppImage build was
  fixed via `NO_STRIP=true` — background in
  [#2](https://github.com/ks98/VoiceTypeX/issues/2). The in-app
  auto-updater (AppImage) is still **disabled** (`includeUpdaterJson: false`)
  until the `AppImage Validate` workflow confirms a launching AppImage.
- **Windows** (NSIS): **in the release** — STT (whisper.cpp + Vulkan) +
  Cloud/Ollama LLM. The embedded llama-cpp-2 has been removed on Windows
  ([#1](https://github.com/ks98/VoiceTypeX/issues/1): ggml symbol collision
  between the two ggml copies when MSVC links), which lets the link
  succeed; CI builds and tests Windows fully (`cargo build + test`). The
  NSIS auto-updater is wired up; the `latest.json` is — as with AppImage —
  still disabled (see above).
- **macOS**: out of scope.

## Initial setup checklist (one-time)

1. `gh auth login` (scopes `repo`, `workflow`).
2. Set the GitHub secret `TAURI_SIGNING_PRIVATE_KEY`:
   `gh secret set TAURI_SIGNING_PRIVATE_KEY --repo ks98/VoiceTypeX < ~/.tauri/voicetypex-updater.key`
3. Optional: mark `v*` as a *Protected Tag* (Settings → Tags) so that only
   maintainers can cut releases.
