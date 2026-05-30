<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Security Model — VoiceTypeX

Diese Datei beschreibt das Bedrohungsmodell, die getroffenen Massnahmen
und die bekannten Grenzen der aktuellen Beta-Version.

## Threat Model

**In-Scope:**

- **Disk-Forensics ohne User-Account-Zugriff** — gestohlenes Notebook,
  Backup-Image auf einem Cloud-Drive, andere User auf demselben System.
  Adressiert via Encryption-at-rest fuer API-Keys (siehe unten).
- **Cloud-Backup-Sync von `~/.config/`** (Dropbox/iCloud/Syncthing).
  API-Keys liegen dort verschluesselt; auf einem Linux-Setup ohne
  Keyring liegen sie im Plaintext-Fallback (UI warnt den User).
- **MITM-Proxy oder fehl-konfiguriertes Enterprise-Gateway** zwischen
  App und Cloud-Provider. Verhindert via TLS (rustls in `reqwest`),
  SHA-256-gepinnte Modell-Downloads, und Filterung von Provider-
  Response-Bodies vor dem User-sichtbaren Log/UI.
- **Versehentliches Loggen sensibler Daten** durch zukuenftige Code-
  Aenderungen. Der `LogRingBuffer` zeichnet die `tracing`-Events
  **ungefiltert** auf — er erzwingt also keine Redaction. Dass keine
  Audio-Bytes, Transkripte, LLM-Antworten oder API-Keys im
  user-sichtbaren Logs-Tab landen, ist heute eine **Konvention** des
  Backend-Codes (die Provider-Clients loggen nur Status-Codes,
  Provider-Namen und Dauer-Metriken), nicht eine technisch erzwungene
  Garantie. Eine feldbasierte Redaction im `StringVisitor` ist als
  Haerte-Schritt vorgemerkt.
- **XSS-Eskalation in einem Webview** (z.B. ueber einen zukuenftigen
  Markdown-Renderer im Logs-Tab). Adressiert via Content-Security-
  Policy mit `script-src 'self'` und expliziter `connect-src`-
  Allowlist; ein XSS-Bug kann die IPC-Schnittstelle nicht erreichen.

**Out-of-Scope (Limitations):**

- **Kompromittierter User-Account oder Root**. Wer den User-Account
  hat, kann auch den OS-Keyring lesen — lokale Crypto schuetzt nicht
  gegen Privilege-Bypass durch den eigentlichen Owner.
- **Memory-Dump des laufenden Prozesses**. API-Keys liegen waehrend
  des Betriebs als `String` im RAM (kein `secrecy`/`zeroize`). Geplant
  fuer 1.0.
- **Repository-Compromise auf Hugging Face**. SHA-256-Pinning haengt
  vom in `src-tauri/src/transcription/model_downloader.rs` eingetragenen
  Hash ab; wenn Hugging Face manipulierte Modelle mit identischer
  Pruefsumme ausliefern wuerde, wuerden wir es nicht merken. Mitigiert
  durch reputable HF-Repos (`ggerganov/whisper.cpp`, `ggml-org/whisper-vad`)
  und Pinning auf konkrete File-Hashes.
- **macOS**. Liegt nicht im Beta-Scope. Wer die App auf macOS startet,
  faellt im Secret-Storage auf Plaintext zurueck (Code-FIXME zeigt auf
  die `Security.framework`-Integration fuer 1.0).
- **Compromised Compositor unter Wayland**. Der persistente
  `restore_token` fuer libei-Tastatur-Inject liegt mit chmod 0600 auf
  Disk. Wer das File lesen kann, kann den Token gegen denselben
  Compositor replayen — das ist gewollt, sonst funktioniert
  Auto-Paste nach App-Restart nicht ohne erneuten Permission-Dialog.

## Secret Storage at Rest

API-Keys (BYOK: xAI, OpenAI, Anthropic, Groq, Deepgram) liegen in
`~/.config/de.kevin-stenzel.voicetypex/secrets.json` (`chmod 0600`).
Das File hat ein versionsiertes JSON-Format und ist
plattformabhaengig verschluesselt:

| Plattform | Methode | Schluessel-Material |
|-----------|---------|---------------------|
| Windows   | DPAPI (`CryptProtectData`) | User- und Maschinen-gebunden |
| Linux     | AES-256-GCM | 32-Byte-Random-KEK im OS-Keyring (libsecret / kwallet) |
| Linux ohne Keyring | Plaintext + UI-Warning | — |
| macOS     | Plaintext + Warning (Beta) | siehe FIXME im Code |

Schluessel-Migration:

- Pre-Beta-Files (v1, flach `{"provider":"key"}`) werden beim ersten
  Start nach Update erkannt und sofort mit der aktiven Verschluesselung
  ueberschrieben.
- Wechsel von Plain auf AES-GCM (z.B. nach Keyring-Installation):
  wird ebenfalls automatisch migriert.
- Wechsel von AES-GCM/DPAPI auf Plain (z.B. Keyring nach System-Wechsel
  nicht mehr verfuegbar): Backend meldet klare Fehlermeldung, Store
  startet leer; User muss API-Keys neu eingeben. **Kein stillschweigender
  Datenverlust**.

Tests in `src-tauri/src/secrets.rs::tests` decken AES-GCM-Roundtrip,
falsche-KEK-Rejection, kurze-Blob-Rejection, v1-Format-Erkennung und
die Stabilitaet der On-Disk-Method-Namen ab.

## Transport Layer Security

- **rustls** statt openSSL fuer `reqwest`-Clients (`reqwest = { ...,
  features = ["rustls-tls", ...] }`).
- **TLS-Cert-Pinning** ist NICHT aktiv. Wir verlassen uns auf das System-
  CA-Bundle. Korrumpierte Enterprise-Root-CAs koennen MITM machen — als
  Mitigation prueft jedes Modell zusaetzlich seinen SHA-256.
- **HTTP-Timeouts** sind an allen sechs Cloud-Provider-Konstruktoren
  gesetzt (60-300 s). `reqwest`-Builder-Fehler panicen frueh statt
  silent einen timeout-losen Fallback-Client zu erzeugen.

## Content Security Policy

Auf allen drei Webviews (`main`, `overlay`, `menu`) aktiv:

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

`unsafe-inline` ist nur fuer `style-src` aktiv (Tailwind/React inline
styles). `script-src` ohne `unsafe-inline`, kein eval.

## Capabilities (Tauri Allowlist)

Die `default`-Capability nutzt die `*:default`-Permission-Sets der
Plugins. Diese sind **enger, als der Begriff „liberal" vermuten
laesst**: `fs:default` etwa gewaehrt in `tauri-plugin-fs` 2.5.x nur
**lesenden** Zugriff auf die App-Verzeichnisse (AppConfig/AppData),
keinen Schreibzugriff und keinen Zugriff ausserhalb. Verbleibendes
Restrisiko: `secrets.json` liegt in genau diesem App-Config-Verzeichnis
und ist damit fuer einen Webview-Kontext prinzipiell lesbar — bei
aktiver Encryption-at-rest aber nur als Chiffrat (auf Linux ohne
Keyring bzw. macOS als Klartext). Die zweite Haerte-Phase vor 1.0
reduziert auf konkrete `allow-*`-Permissions und setzt `secrets.json`
per `fs:scope` auf die Deny-Liste. Die Window-Allowlist
`["main", "overlay", "menu"]` ist explizit und vollstaendig.

## Bug Reports

- **Standard-Bugs**: [GitHub Issues](https://github.com/ks98/voicetypex/issues)
- **Security-Bugs**: bitte direkt an `mail@kevin-stenzel.de`,
  mit Subject-Prefix `[VoiceTypeX-Security]`. Keine
  oeffentlichen Issues fuer unfixierte Vulns.

## Update Path

Beta-Releases haben **keinen** Auto-Updater. Sicherheitsupdates
erfordern manuelles Re-Download vom GitHub-Releases-Tab. Geplant fuer
1.0: signierter Auto-Updater via `tauri-plugin-updater` mit
GPG-signierten Release-Bundles.

## Audit-Stand

Diese Beta wurde vor Release durch zwei interne Audits geprueft
(Architektur + Security). Findings >= HIGH sind adressiert.
Verbleibende MEDIUM/LOW Items sind als Backlog dokumentiert.
