# Modi schreiben

Ein **Modus** ist eine TOML-Datei in `app_config_dir/modes/`. Beim ersten
Start kopiert VoiceTypeX die 6 mitgelieferten Defaults dort hin.
Eigene Modi einfach als weiteres `*.toml` ablegen — der Hot-Reload nimmt
es ohne App-Neustart auf.

## Mitgelieferte Standard-Modi

| Modus | STT | LLM-Postprocessing | Datei |
|---|---|---|---|
| Exaktes Diktat | Lokal (whisper-rs) | — | `exaktes_diktat.toml` |
| Korrigierendes Diktat | Lokal (whisper-rs) | Lokal (Embedded oder Ollama) | `korrigierendes_diktat.toml` |
| Förmliche E-Mail | xAI | xAI Grok-fast | `foermliche_email.toml` |
| Slack/Teams Nachricht | xAI | xAI Grok-fast | `slack_teams.toml` |
| GitHub Issue | xAI | xAI Grok-fast | `github_issue.toml` |
| Anweisung an Coding-Agent | xAI (Auto-Sprache) | xAI Grok-fast | `claude_code_anweisung.toml` |

Modi haben keinen eigenen Hotkey — siehe Abschnitt *Hotkey-Modell*
weiter unten.

**Welcher Modus wann?**
- *Exaktes Diktat* — wenn die Worte 1:1 so wie gesprochen ankommen sollen
  (z.B. Zitate, technische Begriffe, eigene Code-Identifier). Reiner
  Lokal-Pfad, kein Netz.
- *Korrigierendes Diktat* — vollständig offline, lokales LLM entfernt
  Versprecher und Selbstkorrekturen, behält Inhalt 1:1 bei.
- *Förmliche E-Mail* / *Slack/Teams Nachricht* / *GitHub Issue* /
  *Anweisung an Coding-Agent* — Cloud-Pipeline mit Zielton pro Kontext.
  Diese vier teilen die STT-Stage (xAI), unterscheiden sich in
  `system_prompt`, Sampling-Profil und (bei den beiden technischen
  Modi) im Whisper-Glossar via `initial_prompt`. Der Coding-Agent-
  Modus verzichtet bewusst auf `language`, damit Whisper bei sprach-
  gemischten Diktaten („die Funktion `parseConfig` returnt…")
  auto-detect macht.

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
transcription = "local"        # "local" | "cloud"
processing = "none"            # "none" | "local" | "cloud"
```

Es gibt **kein** Hotkey-Feld pro Modus mehr. Modi werden zur Laufzeit
über das Overlay-Menü ausgewählt (siehe *Hotkey-Modell* weiter unten).
Bestehende TOMLs aus älteren Versionen behalten das `hotkey`-Feld —
es wird beim Laden akzeptiert und ignoriert.

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
local_engine = "embedded"       # "embedded" (llama-cpp-2) | "ollama" (extern)
                                # Default bei null/weglassen: "ollama" (Backward-Compat)

# Nur wenn local_engine = "ollama":
ollama_model_tag = "qwen2.5:7b" # Ollama-Model-Tag
# (Deprecated-Alias `local_llm_model` wird beim Load automatisch
#  nach `ollama_model_tag` migriert.)

# Nur wenn local_engine = "embedded" — Override des globalen GGUF-Slots:
embedded_llm_slot = "gemma4-e4b-it-q5_k_m"  # null = globaler Default

# Nur wenn transcription = "local" — Override des globalen Whisper-Slots:
whisper_model_slot = "large-v3-turbo-q8_0"  # null = globaler Default
initial_prompt = """
Optionaler Whisper-Glossar — Eigennamen, Fachbegriffe oder Schreibweisen,
die der Decoder als Kontext bekommen soll.
"""

# Wenn processing != "none" — System-Prompt für die LLM-Nachbearbeitung:
system_prompt = """
Mehrzeiliger Prompt-Text. Wird an das LLM als System-Message geschickt,
das gesprochene Diktat als User-Message.
"""

# Optional — Sampling-Parameter (sonst engine-/provider-spezifischer Default):
temperature = 0.3       # 0.0 – 2.0
top_p = 0.8             # 0.0 – 1.0
repeat_penalty = 1.05   # 0.5 – 2.0
max_tokens = 1024       # 1 – 8192
```

## Validierung

VoiceTypeX prüft beim Laden:

- **`id`**: nicht leer, keine Whitespaces, eindeutig.
- **`transcription = "cloud"`**: erfordert `cloud_stt_provider`.
- **`processing = "cloud"`**: erfordert `cloud_llm_provider`.
- **`processing != "none"`**: erfordert `system_prompt`.
- **`local_engine`**: nur `"embedded"` oder `"ollama"`.
- **`processing = "local"` + `local_engine = "ollama"`**: erfordert
  `ollama_model_tag` (oder Deprecated-`local_llm_model`).
- **Sampling-Ranges**: `temperature ∈ [0.0, 2.0]`, `top_p ∈ [0.0, 1.0]`,
  `repeat_penalty ∈ [0.5, 2.0]`, `max_tokens ∈ [1, 8192]`.

Fehlerhafte Modi werden komplett verworfen (das ganze Verzeichnis-Reload
schlägt fehl) — die zuvor geladenen Modi bleiben aktiv. Der Fehler
erscheint im UI-Logs-View.

## Hotkey-Modell

Es gibt **genau einen** globalen Hotkey für die ganze App
(Settings: `menu_hotkey`, Default `CommandOrControl+Alt+Space`):

1. **`Idle` + Hotkey** → Overlay zeigt die Modus-Liste; `↑`/`↓` wählen,
   `Enter` startet die Aufnahme, `Esc` bricht ab. Der Cursor steht
   initial auf dem zuletzt gewählten Modus
   (`Settings.last_selected_mode_id`).
2. **`Recording` + Hotkey** → laufende Aufnahme wird mit dem beim Start
   gewählten Modus finalisiert (Toggle-Stop, gleicher Hotkey).
3. Andere Pipeline-Zustände (Transcribing/Postprocessing/Injecting)
   ignorieren den Hotkey.

### Hotkey-Format

`tauri-plugin-global-shortcut`-kompatibel:
- Modifier: `CommandOrControl`, `Alt`, `Shift`, `Super`/`Meta`
- Key: `A`-`Z`, `0`-`9`, `F1`-`F24`, `Space`, `Tab`, `Enter`, …
- Beispiele: `"CommandOrControl+Alt+Space"`, `"Super+Space"`,
  `"Control+Shift+F12"`

`CommandOrControl` wird auf macOS als `Cmd` interpretiert, sonst als
`Ctrl`. Auf Wayland ist der Wert nur ein Vorschlag — der Compositor
zeigt beim ersten Start einen Dialog zur finalen Zuweisung
(`xdg-desktop-portal.GlobalShortcuts`).

### Wayland: Hotkey-Anzeige weicht von der Einstellung ab

Wenn du auf KDE in *System-Settings → Globale Verknüpfungen →
VoiceTypeX* einen anderen Hotkey zuweist, ist das die effektive
Bindung — VoiceTypeX nimmt die `Settings.menu_hotkey`-Eingabe beim
zweiten Start nicht mehr an, weil KDE die User-Zuweisung priorisiert.
Die Einstellungsseite zeigt deshalb auf Wayland ein read-only Feld
mit dem tatsächlich gebundenen Trigger; ändern geht nur über die
KDE-System-Settings. Auf X11 / Windows ist die App-Einstellung
weiterhin die Wahrheit (editierbar).

## Beispiel: Nutzer-eigener Modus

```toml
# ~/.config/de.kevin-stenzel.voicetypex/modes/sql_review.toml
id = "sql"
name = "SQL Review-Kommentar"
description = "Diktat -> kompakter SQL-Review-Kommentar fuer Pull-Requests."
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
Datei und nimmt den Modus sofort ins Overlay-Menü auf.
