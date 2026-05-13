# Cloud-Provider — Wire-Protokolle

> Stand: Mai 2026. Bei API-Drift vor Implementierungs-Änderungen die
> offizielle Provider-Doku via WebFetch ziehen — diese Datei ist eine
> Momentaufnahme, keine Zukunfts-Garantie. Drittquellen (Blog-Posts,
> Forum-Threads) sind nur Hinweise, nie Ersatz.

## Überblick

| Domäne | Provider | Datei im Code |
|---|---|---|
| STT | xAI | `src-tauri/src/transcription/cloud/xai.rs` |
| STT | OpenAI Whisper | `src-tauri/src/transcription/cloud/openai.rs` (Wrap um `whisper_compatible.rs`) |
| STT | Groq Whisper | `src-tauri/src/transcription/cloud/groq.rs` (Wrap um `whisper_compatible.rs`) |
| STT | Deepgram | `src-tauri/src/transcription/cloud/deepgram.rs` |
| LLM | xAI Grok | `src-tauri/src/processing/cloud/xai.rs` (Wrap um `openai_compatible.rs`) |
| LLM | OpenAI GPT | `src-tauri/src/processing/cloud/openai.rs` (Wrap um `openai_compatible.rs`) |
| LLM | Anthropic Claude | `src-tauri/src/processing/cloud/anthropic.rs` |
| LLM (lokal) | Ollama | `src-tauri/src/processing/local.rs` |

Die Wahl, wann ein Wrapper geteilt wird und wann nicht, folgt der
realen Protokoll-Verwandtschaft (CLAUDE.md §4.6): OpenAI und Groq
sind beide Whisper-API-kompatibel und teilen `whisper_compatible.rs`;
xAI, OpenAI und Groq teilen sich Chat-Completions-kompatibel über
`openai_compatible.rs`. Deepgram (STT) und Anthropic (LLM) sind
eigenständig — kein künstlicher Shared-Wrapper.

## STT-Provider

### xAI STT

- **Endpoint:** `POST https://api.x.ai/v1/stt`
- **Auth:** Bearer-Header (`Authorization: Bearer <api_key>`)
- **Body:** `multipart/form-data`
- **Wichtig:** `file` muss das **letzte** Multipart-Feld sein.
  Andere Felder (z.B. `model`, `response_format`) davor.
- **Response:** `{ text, language, duration, words[] }` — wir nutzen
  nur `text`.
- **Sprach-Erzwingung:** keine. xAI's `language`-Parameter steuert nur
  Text-Formatting (Zahlen/Währungen), nicht die Spracherkennung.
  Die Erkennung ist hartcodiert auto-detect — siehe Abschnitt
  „Bekannte Limitierungen" unten.

### OpenAI Whisper / Groq Whisper

Beide nutzen die Whisper-API von OpenAI bzw. Groqs API-kompatible Variante.
Gemeinsame Implementierung in `whisper_compatible.rs`:

- **Endpoint (OpenAI):** `POST https://api.openai.com/v1/audio/transcriptions`
- **Endpoint (Groq):** `POST https://api.groq.com/openai/v1/audio/transcriptions`
- **Auth:** Bearer-Header
- **Body:** `multipart/form-data` mit `file` und `model`
- **Model:** OpenAI `whisper-1`, Groq `whisper-large-v3-turbo`
- **Response:** `{ text }` (json-Format)

### Deepgram

- **Endpoint:** `POST https://api.deepgram.com/v1/listen?model=nova-2&language=…`
- **Auth:** `Authorization: Token <api_key>` (**nicht** Bearer)
- **Body:** Raw-Audio-Bytes (Content-Type passend zum WAV)
- **Response:** `{ results: { channels: [ { alternatives: [ { transcript } ] } ] } }`

## LLM-Provider

### xAI Grok / OpenAI GPT — OpenAI-Chat-Completions-kompatibel

Gemeinsame Implementierung in `openai_compatible.rs`:

- **Endpoint-Suffix:** `POST {base_url}/chat/completions`
- **Base-URLs:** xAI `https://api.x.ai/v1`, OpenAI `https://api.openai.com/v1`
- **Auth:** Bearer-Header
- **Body:**
  ```json
  {
    "model": "...",
    "messages": [
      { "role": "system", "content": "<system_prompt>" },
      { "role": "user",   "content": "<transcript>" }
    ]
  }
  ```
- **Response-Pfad:** `choices[0].message.content`
- **Default-Models:**
  - xAI: `grok-4-fast-non-reasoning` (Postprocessing-Default —
    kein Reasoning-Overhead, ~6× günstiger als `grok-4`, 2 M Context).
    `grok-4` nur opt-in pro Modus, wenn echtes Multi-Step-Reasoning
    gebraucht wird.
  - OpenAI: `gpt-4o-mini`.

### Anthropic Claude — eigenständig

Anthropic nutzt die Messages-API, nicht Chat-Completions:

- **Endpoint:** `POST https://api.anthropic.com/v1/messages`
- **Auth:** `x-api-key: <api_key>` (**nicht** Bearer)
- **Pflicht-Header:** `anthropic-version: 2023-06-01`
- **Body:**
  ```json
  {
    "model": "...",
    "system": "<system_prompt>",
    "messages": [
      { "role": "user", "content": "<transcript>" }
    ],
    "max_tokens": 4096
  }
  ```
  - **Achtung:** `system` ist Top-Level-Feld, **nicht** Teil der
    `messages`-Liste (anders als bei OpenAI-Kompatiblen).
- **Response-Pfad:** `content[0].text`

### Ollama (lokal, kein BYOK-Key)

- **Endpoint (Default):** `POST http://127.0.0.1:11434/api/chat`
- **Auth:** keine (lokaler HTTP-Server)
- **Body:** Chat-Format analog zu OpenAI:
  ```json
  {
    "model": "...",
    "messages": [
      { "role": "system", "content": "<system_prompt>" },
      { "role": "user",   "content": "<transcript>" }
    ],
    "stream": false,
    "options": { "temperature": 0.2 }
  }
  ```
- **Response-Pfad:** `message.content`
- **Timeout:** 300 s (lokale Inferenz kann auf CPU dauern)
- **Endpoint überschreibbar:** Settings-Feld `ollama_url`.

## Secret-Handling

API-Keys sind pro Provider im File `~/.config/.../secrets.json` (chmod
0600) plus best-effort im OS-Keychain. **xAI ist ein Sonderfall:** ein
einziger Eintrag deckt STT *und* LLM ab, weil beide Endpoints denselben
Key nutzen.

Keys werden **niemals** geloggt (siehe CLAUDE.md §8) — Logging zeigt
nur die Key-Länge zur Diagnostik. Provider-Requests gehen
ausschließlich durch das Rust-Backend; der Key verlässt den Prozess
nicht ins Frontend (IPC `get_provider_status` liefert nur
`{ configured: bool, error: Option<String> }`).

## Bekannte Limitierungen

### xAI STT — keine Sprach-Erzwingung

xAI's STT-API akzeptiert keinen Parameter, mit dem sich die Sprach-
Erkennung festnageln ließe. Das `language`-Feld in der Request steuert
nur Text-Formatting (z.B. Schreibweise von Zahlen und Währungen),
nicht die akustische Spracherkennung. Die Erkennung ist serverseitig
hartcodiert auto-detect.

**Praktische Konsequenz:** Bei kurzen, sprachneutralen Diktaten
(z.B. einzelne Eigennamen, technische Begriffe, kurze Befehle) kann
das Modell daneben raten — z.B. einen deutschen Befehl als englisch
interpretieren und phonetisch transkribieren.

**Workaround:** Auf lokales Whisper-STT (`transcription = "local"` im
Modus) wechseln; dort lässt sich `language = "de"` erzwingen. Für
längere, klar deutschsprachige Diktate ist xAI in der Praxis robust,
deshalb akzeptieren wir das Limit für die Cloud-Modi und planen
**keinen** Fix (würde Provider-API-seitig erfordern und ist nicht in
unserer Hand).

**Quellen:** Verhalten beobachtet in Eigenpraxis (Stand Mai 2026); xAI
hat das Verhalten nicht offiziell als API-Constraint dokumentiert, also
ist eine spätere Änderung jederzeit möglich.

## Wenn du einen neuen Provider einbaust

1. Offizielle Provider-Doku via WebFetch ziehen — nicht auf diese
   Datei oder ähnlich-aussehende APIs verlassen.
2. Prüfen, ob er strukturell zu einem bestehenden Wrapper passt
   (`whisper_compatible.rs` für Whisper-API-kompatible STT,
   `openai_compatible.rs` für Chat-Completions-kompatible LLM).
   **Nur** dann in den Wrapper aufnehmen, wenn die Verwandtschaft real
   ist — sonst eigenständige Datei wie Deepgram / Anthropic.
3. Auth-Modus genau anschauen: Bearer ist üblich, aber Deepgram nutzt
   `Token`, Anthropic `x-api-key`. Pflicht-Header (`anthropic-version`)
   beachten.
4. Secret-Eintrag in `core/config.rs` ergänzen + Factory in
   `transcription/mod.rs` bzw. `processing/mod.rs` erweitern.
5. Tests für Response-Parsing (Mock-Response → `text`-Extraktion).
