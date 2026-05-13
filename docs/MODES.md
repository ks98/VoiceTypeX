# Modi schreiben

Ein **Modus** ist eine TOML-Datei in `app_config_dir/modes/`. Beim ersten
Start kopiert VoiceTypeX die 6 mitgelieferten Defaults dort hin.
Eigene Modi einfach als weiteres `*.toml` ablegen — der Hot-Reload nimmt
es ohne App-Neustart auf.

## Mitgelieferte Standard-Modi

| Modus | Hotkey | STT | LLM-Postprocessing | Datei |
|---|---|---|---|---|
| Exaktes Diktat | `Ctrl+Alt+D` | Lokal (whisper-rs) | — | `exaktes_diktat.toml` |
| Korrigierendes Diktat | `Ctrl+Alt+K` | Lokal (whisper-rs) | Lokal (Ollama) | `korrigierendes_diktat.toml` |
| Förmliche E-Mail | `Ctrl+Alt+E` | xAI | xAI Grok-fast | `foermliche_email.toml` |
| Slack/Teams | `Ctrl+Alt+S` | xAI | xAI Grok-fast | `slack_teams.toml` |
| GitHub-Issue | `Ctrl+Alt+G` | xAI | xAI Grok-fast | `github_issue.toml` |
| Claude-Code-Anweisung | `Ctrl+Alt+C` | xAI | xAI Grok-fast | `claude_code_anweisung.toml` |

**Welcher Modus wann?**
- *Exaktes Diktat* — wenn die Worte 1:1 so wie gesprochen ankommen sollen
  (z.B. Zitate, technische Begriffe, eigene Code-Identifier).
- *Korrigierendes Diktat* — vollständig offline, mit lokalem LLM zur
  Glättung von Verhasplern und Füllwörtern.
- *Förmliche E-Mail* / *Slack* / *GitHub-Issue* / *Claude-Code* —
  Cloud-Pipeline mit Zielton pro Kontext. Diese vier teilen die
  STT-Stage (xAI), unterscheiden sich nur im `system_prompt` des
  Postprocessings.

## Speicherort

| OS | Pfad |
|---|---|
| Linux | `~/.config/de.kevin-stenzel.voicetypex/modes/` |
| Windows | `%APPDATA%\de.kevin-stenzel.voicetypex\modes\` |
| macOS | `~/Library/Application Support/de.kevin-stenzel.voicetypex/modes/` |

## Pflichtfelder

```toml
id = "kurz_ohne_spaces"        # eindeutig, keine Whitespaces
name = "Anzeigename"           # erscheint im UI
hotkey = "CommandOrControl+Alt+D"  # globaler Shortcut
transcription = "local"        # "local" | "cloud"
processing = "none"            # "none" | "local" | "cloud"
```

## Optional

```toml
description = "Kurze Beschreibung was der Modus tut."
language = "de"                # ISO-Code, Hint für STT
injection_method = "clipboard" # "clipboard" (Default) | "keystrokes"

# Nur wenn transcription = "cloud":
cloud_stt_provider = "xai"     # "xai" | "openai" | "groq" | "deepgram"

# Nur wenn processing = "cloud":
cloud_llm_provider = "xai"     # "xai" | "openai" | "anthropic"
cloud_llm_model = "grok-4-fast-non-reasoning"
                               # provider-spezifischer Model-Identifier
                               # xAI-Default ist grok-4-fast-non-reasoning
                               # (kein Reasoning-Overhead, ~6× günstiger
                               # als grok-4 bei Postprocessing-Aufgaben)

# Nur wenn processing = "local":
local_llm_model = "qwen2.5:7b" # Ollama-Model-Tag

# Wenn processing != "none" — System-Prompt für die LLM-Nachbearbeitung:
system_prompt = """
Mehrzeiliger Prompt-Text. Wird an das LLM als System-Message geschickt,
das gesprochene Diktat als User-Message.
"""
```

## Validierung

VoiceTypeX prüft beim Laden:

- **`id`**: nicht leer, keine Whitespaces.
- **Hotkey-Konflikte**: kein zweiter Modus darf denselben Hotkey nutzen.
- **`transcription = "cloud"`**: erfordert `cloud_stt_provider`.
- **`processing = "cloud"`**: erfordert `cloud_llm_provider`.
- **`processing != "none"`**: erfordert `system_prompt`.

Fehlerhafte Modi werden komplett verworfen (das ganze Verzeichnis-Reload
schlägt fehl) — die zuvor geladenen Modi bleiben aktiv. Der Fehler
erscheint im UI-Logs-View.

## Hotkey-Format

`tauri-plugin-global-shortcut`-kompatibel:
- Modifier: `CommandOrControl`, `Alt`, `Shift`, `Super`/`Meta`
- Key: `A`-`Z`, `0`-`9`, `F1`-`F24`, `Space`, `Tab`, `Enter`, …
- Beispiele: `"CommandOrControl+Alt+D"`, `"Super+Space"`,
  `"Control+Shift+F12"`

`CommandOrControl` wird auf macOS als `Cmd` interpretiert, sonst als
`Ctrl`.

## Push-to-Talk vs. Toggle

Der Hotkey-Modus ist global konfiguriert (Settings: `ptt_mode`), nicht
pro Modus:

- **Push-to-Talk (Default):** Hotkey **gedrückt halten** = Aufnahme,
  loslassen = Pipeline läuft an.
- **Toggle:** Hotkey **drücken** = Aufnahme an, **drücken** = aus. Nützlich
  als Fallback für Wayland-Compositors mit unzuverlässigem Release-
  Signal.

## Beispiel: Nutzer-eigener Modus

```toml
# ~/.config/de.kevin-stenzel.voicetypex/modes/sql_review.toml
id = "sql"
name = "SQL Review-Kommentar"
description = "Diktat -> kompakter SQL-Review-Kommentar fuer Pull-Requests."
hotkey = "CommandOrControl+Alt+Q"
transcription = "cloud"
processing = "cloud"
cloud_stt_provider = "xai"
cloud_llm_provider = "xai"
cloud_llm_model = "grok-4-fast-non-reasoning"
language = "de"
injection_method = "clipboard"
system_prompt = """
Du bekommst ein gesprochenes Diktat eines SQL-Reviewers.
Forme es zu einem klaren, technisch-knappen Code-Review-Kommentar:
- Eine Zeile als Kernpunkt.
- Optional 2-3 Zeilen Begruendung mit Index-/Plan-Hinweisen.
- Wenn der Sprecher konkretes SQL nennt, in `Backticks` einbetten.
- Sprache: Deutsch, technisch, ohne Floskeln.
- Gib NUR den Kommentar aus.
"""
```

Speichern, fertig. App-Neustart unnötig — der Hot-Reload sieht die neue
Datei und registriert den Hotkey.
