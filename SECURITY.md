<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Security Model — VoiceTypeX

This file describes the threat model, the mitigations in place, and the
known limitations of the current beta version.

## Threat Model

**In-Scope:**

- **Disk forensics without user-account access** — a stolen laptop, a
  backup image on a cloud drive, or other users on the same system.
  Addressed via encryption-at-rest for API keys (see below).
- **Cloud backup sync of `~/.config/`** (Dropbox/iCloud/Syncthing).
  API keys are stored there encrypted; on a Linux setup without a
  keyring they fall back to plaintext (the UI warns the user).
- **MITM proxy or misconfigured enterprise gateway** between the app and
  the cloud provider. Prevented via TLS (rustls in `reqwest`),
  SHA-256-pinned model downloads, and filtering of provider response
  bodies before they reach the user-visible log/UI.
- **Accidental logging of sensitive data** through future code changes.
  The `LogRingBuffer` records `tracing` events **unfiltered** — so it
  does not enforce redaction. The fact that no audio bytes, transcripts,
  LLM responses, or API keys end up in the user-visible Logs tab is today
  a **convention** of the backend code (the provider clients log only
  status codes, provider names, and timing metrics), not a
  technically enforced guarantee. Field-based redaction in the
  `StringVisitor` is planned as a hardening step.
- **XSS escalation in a webview** (e.g. via a future Markdown renderer in
  the Logs tab). Addressed via a Content Security Policy with
  `script-src 'self'` and an explicit `connect-src` allowlist; an XSS
  bug cannot reach the IPC interface.

**Out-of-Scope (Limitations):**

- **Compromised user account or root**. Anyone who has the user account
  can also read the OS keyring — local crypto does not protect against a
  privilege bypass by the actual owner.
- **Memory dump of the running process**. API keys live in RAM as a
  `String` during operation (no `secrecy`/`zeroize`). Planned for 1.0.
- **Repository compromise on Hugging Face**. SHA-256 pinning depends on
  the hash recorded in
  `src-tauri/src/transcription/model_downloader.rs`; if Hugging Face were
  to serve tampered models with an identical checksum, we would not
  notice. Mitigated by using reputable HF repos
  (`ggerganov/whisper.cpp`, `ggml-org/whisper-vad`) and pinning to
  specific file hashes.
- **macOS**. Outside the beta scope. Anyone who launches the app on macOS
  falls back to plaintext for secret storage (a code FIXME points to the
  `Security.framework` integration for 1.0).
- **Compromised compositor under Wayland**. The persistent
  `restore_token` for libei keyboard injection lives on disk with chmod
  0600. Anyone who can read the file can replay the token against the
  same compositor — this is intentional, otherwise auto-paste would not
  work after an app restart without another permission dialog.

## Secret Storage at Rest

API keys (BYOK: xAI, OpenAI, Anthropic, Groq, Deepgram) are stored in
`~/.config/de.kevin-stenzel.voicetypex/secrets.json` (`chmod 0600`).
The file uses a versioned JSON format and is encrypted in a
platform-dependent way:

| Platform  | Method  | Key material        |
|-----------|---------|---------------------|
| Windows   | DPAPI (`CryptProtectData`) | User- and machine-bound |
| Linux     | AES-256-GCM | 32-byte random KEK in the OS keyring (libsecret / kwallet) |
| Linux without keyring | Plaintext + UI warning | — |
| macOS     | Plaintext + warning (beta) | see FIXME in the code |

Key migration:

- Pre-beta files (v1, flat `{"provider":"key"}`) are detected on the
  first launch after an update and immediately overwritten with the
  active encryption.
- Switching from plaintext to AES-GCM (e.g. after installing a keyring):
  also migrated automatically.
- Switching from AES-GCM/DPAPI to plaintext (e.g. the keyring is no
  longer available after a system change): the backend returns a clear
  error message, the store starts empty, and the user must re-enter the
  API keys. **No silent data loss**.

Tests in `src-tauri/src/secrets.rs::tests` cover the AES-GCM round trip,
wrong-KEK rejection, short-blob rejection, v1-format detection, and the
stability of the on-disk method names.

## Transport Layer Security

- **rustls** instead of OpenSSL for `reqwest` clients (`reqwest = { ...,
  features = ["rustls-tls", ...] }`).
- **TLS cert pinning** is NOT active. We rely on the system CA bundle.
  Corrupted enterprise root CAs can perform MITM — as a mitigation, every
  model additionally verifies its SHA-256.
- **HTTP timeouts** are set on all six cloud-provider constructors
  (60–300 s). `reqwest` builder errors panic early instead of silently
  creating a timeout-less fallback client.

## Content Security Policy

Active on all three webviews (`main`, `overlay`, `menu`):

```
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';
img-src 'self' data: asset: https://asset.localhost;
font-src 'self' data:;
connect-src 'self' ipc: http://ipc.localhost
            https://api.anthropic.com https://api.openai.com
            https://api.x.ai https://api.groq.com https://api.deepgram.com
            https://huggingface.co https://*.huggingface.co;
```

`unsafe-inline` is enabled only for `style-src` (Tailwind/React inline
styles). `script-src` has no `unsafe-inline`, and no eval.

## Capabilities (Tauri Allowlist)

The `default` capability uses the plugins' `*:default` permission sets.
These are **narrower than the term "liberal" suggests**: `fs:default`,
for example, grants only **read** access to the app directories
(AppConfig/AppData) in `tauri-plugin-fs` 2.5.x — no write access and no
access outside them. Remaining residual risk: `secrets.json` lives in
exactly this app-config directory and is therefore in principle readable
from a webview context — but with encryption-at-rest active only as
ciphertext (as plaintext only on Linux without a keyring, or on macOS).
The second hardening phase before 1.0 reduces this to specific `allow-*`
permissions and puts `secrets.json` on the deny list via `fs:scope`. The
window allowlist `["main", "overlay", "menu"]` is explicit and complete.

## Bug Reports

- **Standard bugs**: [GitHub Issues](https://github.com/ks98/VoiceTypeX/issues)
- **Security bugs**: please email `mail@kevin-stenzel.de` directly, with
  the subject prefix `[VoiceTypeX-Security]`. No public issues for
  unfixed vulnerabilities.

## Update Path

VoiceTypeX has a **signed auto-updater** (*Settings → Diagnostics →
Updates*) for the **Windows NSIS installer** and the **Linux AppImage**:
update artifacts are signed with a minisign/Ed25519 key, and the updater
verifies the signature against the public key embedded in the app before
installing. **`.deb`/`.rpm`** have no in-app updater — they are updated
via the package manager or by re-downloading from the GitHub Releases tab.
The **Windows installer is not yet Authenticode-signed** (SmartScreen
warning on first launch); this affects only the installer download, not
the updater's integrity.

## Audit Status

This beta was reviewed by two internal audits before release
(architecture + security). Findings of severity >= HIGH are addressed.
Remaining MEDIUM/LOW items are documented in the backlog.
