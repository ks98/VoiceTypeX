# Writing Modes

A **mode** is a TOML file in `app_config_dir/modes/`. On first launch,
VoiceTypeX copies the 9 bundled defaults there —
**locale-matched**: if your UI language is `fr` on first launch, you
get French defaults, including translated
`system_prompt`s. The language sets live under
`modes/defaults/{de,en,fr,es,it}/` in the source and are embedded into the binary via
`include_str!` (backend logic in
[`src-tauri/src/core/default_modes.rs`](../src-tauri/src/core/default_modes.rs)).
To add your own modes, just drop another `*.toml` in place — hot-reload picks
it up without restarting the app.

**Later locale switches do NOT affect already-copied modes.**
If you want English defaults after switching to `en` later:
empty the modes/ directory and restart the app → bootstrap copies in the
active-locale defaults. (Your own custom modes are lost in the process —
back them up first.)

## Bundled Standard Modes (example: English locale)

| Mode | STT | LLM Postprocessing | File |
|---|---|---|---|
| Exact dictation | Local (whisper-rs) | — | `exaktes_diktat.toml` |
| Corrective dictation | Local (whisper-rs) | Local (embedded or Ollama) | `korrigierendes_diktat.toml` |
| Formal email | xAI | xAI Grok-fast | `foermliche_email.toml` |
| Slack/Teams message | xAI | xAI Grok-fast | `slack_teams.toml` |
| GitHub issue | xAI | xAI Grok-fast | `github_issue.toml` |
| Coding-agent instruction | xAI (auto language) | xAI Grok-fast | `claude_code_anweisung.toml` |
| Improve *(Edit)* | xAI | xAI Grok-fast | `improve.toml` |
| Write reply *(Edit)* | xAI | xAI Grok-fast | `reply.toml` |
| Free edit *(Edit)* | xAI | xAI Grok-fast | `transform.toml` |

The filenames are identical across all locales (historically chosen in
German). `id`/`name`/`description`/`system_prompt`/`language` and
`initial_prompt` are localized per locale; the STT/LLM/sampling
configuration is identical.

Modes have no hotkey of their own — see the *Hotkey Model* section
below.

**Which mode when?**
- *Exact dictation* — when the words should land exactly as spoken
  (e.g. quotes, technical terms, your own code identifiers). A purely
  local path, no network.
- *Corrective dictation* — fully offline on Linux/macOS (the
  embedded LLM removes slips of the tongue and self-corrections, while keeping the content
  verbatim). **On Windows** the embedded LLM is not compiled
  (Issue #1) — the TOML is identical across platforms, but at runtime
  the mode there requires a self-installed Ollama daemon
  (`local_engine = "ollama"`) or a cloud provider; otherwise it reports
  a clear error when triggered instead of crashing.
- *Formal email* / *Slack/Teams message* / *GitHub issue* /
  *Coding-agent instruction* — cloud pipeline with a target tone per
  context. These four share the STT stage (xAI) and differ in
  `system_prompt`, sampling profile, and (for the two technical
  modes) the Whisper glossary via `initial_prompt`. The Coding-Agent
  mode deliberately omits `language` so that Whisper auto-detects on
  language-mixed dictations ("die Funktion `parseConfig` returnt…").

## Edit Modes (select text → transform)

A mode with `input = "selection"` does not work on fresh dictation,
but on the **currently selected text** in the focused app.
Flow: select text → hotkey → pick an edit mode from the menu
→ optionally speak an instruction → stop hotkey. The selection is read
on hotkey press (before the menu, while the target app is still focused)
via the clipboard; the spoken dictation becomes the
instruction. The LLM receives both as:

```
<selected_text>
… the selected text …
</selected_text>

<instruction>
… the spoken dictation (can be empty) …
</instruction>
```

The `system_prompt` tells the model how to handle the two blocks.
The result is placed according to `output`.

**`output = "auto"`** leaves placement to the LLM: it begins its
response with **exactly one** control line — `@@REPLACE`, `@@APPEND`, or
`@@PREPEND` — followed by the actual text. VoiceTypeX strips the
control line and places accordingly. If it is missing,
`output_fallback` kicks in.

**Eager-capture cost:** the selection is only read if an edit mode
exists at all. Pure dictation setups notice nothing of it.

Platform details and limits (e.g. Wayland clipboard,
`append`/`prepend`) are in [`PLATFORMS.md`](PLATFORMS.md).

## Storage Location

| OS | Path |
|---|---|
| Linux | `~/.config/de.kevin-stenzel.voicetypex/modes/` |
| Windows | `%APPDATA%\de.kevin-stenzel.voicetypex\modes\` |
| macOS | `~/Library/Application Support/de.kevin-stenzel.voicetypex/modes/` |

## Required Fields

```toml
id = "short_no_spaces"         # unique, no whitespace
name = "Display name"          # shown in the UI
transcription = "local"        # "local" | "cloud"
processing = "none"            # "none" | "local" | "cloud"
```

There is **no** per-mode hotkey field anymore. Modes are selected at
runtime via the overlay menu (see *Hotkey Model* below).
Existing TOMLs from older versions keep the `hotkey` field —
it is accepted and ignored on load.

## Optional

```toml
description = "Short description of what the mode does."
language = "de"                # ISO code, hint for STT
injection_method = "clipboard" # "clipboard" (default) | "keystrokes"

# --- Edit modes (transform selected text) ---
input = "voice"        # "voice" (default, dictation) | "selection"
                       # "selection": reads the selected text from the
                       # focused app and uses the dictation as an
                       # (optional) instruction on it. Requires
                       # processing != "none".
output = "insert"      # "insert" (default, at cursor position)
                       # Only meaningful with input="selection":
                       #   "replace" – replaces the selection
                       #   "append"  – keeps the selection, appends below it
                       #   "prepend" – puts the result before it
                       #   "auto"    – the LLM decides (see below)
output_fallback = "replace"  # only with output="auto": action when the
                             # LLM emits no control line. Not "auto".

# Only if transcription = "cloud":
cloud_stt_provider = "xai"     # "xai" | "openai" | "groq" | "deepgram"

# Only if processing = "cloud":
cloud_llm_provider = "xai"     # "xai" | "openai" | "anthropic"
cloud_llm_model = "grok-4-fast-non-reasoning"
                               # provider-specific model identifier
                               # xAI default is grok-4-fast-non-reasoning
                               # (no reasoning overhead, ~6x cheaper
                               # than grok-4 for postprocessing tasks)

# Only if processing = "local":
local_engine = "embedded"       # "embedded" (llama-cpp-2, Linux/macOS) | "ollama" (external daemon)
                                # Default if null/omitted: "embedded" on Linux/macOS, "ollama"
                                # on Windows (embedded is not compiled there — Issue #1).
                                # Old TOMLs with `local_llm_model` (Phase 1/2) are automatically
                                # migrated to "ollama" on load, so they are not
                                # accidentally redirected onto the wrong engine path.

# Only if local_engine = "ollama":
ollama_model_tag = "qwen2.5:7b" # Ollama model tag
# (Deprecated alias `local_llm_model` is automatically migrated
#  to `ollama_model_tag` on load.)

# Only if local_engine = "embedded" — override the global GGUF slot:
embedded_llm_slot = "gemma4-e4b-it-q5_k_m"  # null = global default

# Only if transcription = "local" — override the global Whisper slot:
whisper_model_slot = "large-v3-turbo-q8_0"  # null = global default

# Only if transcription = "local" — override the beam width of the
# final pass. null = global default (Settings.whisper_beam_size, = 5).
# Range 1..=10: lower = faster (1 ≈ greedy), higher = marginally
# more accurate + slower. Cloud STT ignores it.
whisper_beam_size = 5

initial_prompt = """
Optional Whisper glossary — proper nouns, technical terms, or spellings
the decoder should receive as context.
"""

# If processing != "none" — system prompt for the LLM postprocessing:
system_prompt = """
Multi-line prompt text. Sent to the LLM as the system message,
the spoken dictation as the user message.
"""

# Optional — sampling parameters (otherwise engine-/provider-specific default):
temperature = 0.3       # 0.0 – 2.0
top_p = 0.8             # 0.0 – 1.0
repeat_penalty = 1.05   # 0.5 – 2.0
max_tokens = 1024       # 1 – 8192
```

## Validation

On load, VoiceTypeX checks:

- **`id`**: not empty, no whitespace, unique.
- **`transcription = "cloud"`**: requires `cloud_stt_provider`.
- **`processing = "cloud"`**: requires `cloud_llm_provider`.
- **`processing != "none"`**: requires `system_prompt`.
- **`input = "selection"`**: requires `processing != "none"` (without an
  LLM there is nothing to transform the selection with).
- **`output ∈ {replace, append, prepend, auto}`**: only with
  `input = "selection"` (voice modes always insert at the cursor
  position → `output = "insert"`).
- **`output_fallback`**: must not be `"auto"`.
- **`local_engine`**: only `"embedded"` or `"ollama"`.
- **`processing = "local"` + `local_engine = "ollama"`**: requires
  `ollama_model_tag` (or the deprecated `local_llm_model`).
- **Sampling ranges**: `temperature ∈ [0.0, 2.0]`, `top_p ∈ [0.0, 1.0]`,
  `repeat_penalty ∈ [0.5, 2.0]`, `max_tokens ∈ [1, 8192]`.

Faulty modes are discarded entirely (the whole directory reload
fails) — the previously loaded modes stay active. The error
appears in the UI Logs view.

## Hotkey Model

There is **exactly one** global hotkey for the whole app
(Settings: `menu_hotkey`, default `CommandOrControl+Alt+Space`):

1. **`Idle` + hotkey** → the menu window shows the mode list; `↑`/`↓` select,
   `Enter` starts recording, `Esc` cancels. The cursor initially
   rests on the last-selected mode
   (`Settings.last_selected_mode_id`).
2. **`Recording` + hotkey** → the running recording is finalized with the
   mode chosen at start (toggle-stop, same hotkey).
3. Other pipeline states (Transcribing/Postprocessing/Injecting)
   ignore the hotkey.

### Hotkey Format

`tauri-plugin-global-shortcut`-compatible:
- Modifiers: `CommandOrControl`, `Alt`, `Shift`, `Super`/`Meta`
- Key: `A`-`Z`, `0`-`9`, `F1`-`F24`, `Space`, `Tab`, `Enter`, …
- Examples: `"CommandOrControl+Alt+Space"`, `"Super+Space"`,
  `"Control+Shift+F12"`

`CommandOrControl` is interpreted as `Cmd` on macOS, otherwise as
`Ctrl`. On Wayland the value is only a suggestion — the compositor
shows a dialog on first launch for the final binding
(`xdg-desktop-portal.GlobalShortcuts`).

### Wayland: hotkey display differs from the setting

If you assign a different hotkey on KDE under *System Settings → Global
Shortcuts → VoiceTypeX*, that is the effective binding —
VoiceTypeX no longer accepts the `Settings.menu_hotkey` input on the
second launch, because KDE prioritizes the user assignment.
The settings page therefore shows a read-only field on Wayland
with the actually bound trigger; changing it is only possible via the
KDE System Settings. On X11 / Windows the app setting is
still the source of truth (editable).

## Example: a user's own mode

```toml
# ~/.config/de.kevin-stenzel.voicetypex/modes/sql_review.toml
id = "sql"
name = "SQL review comment"
description = "Dictation -> compact SQL review comment for pull requests."
transcription = "cloud"
processing = "cloud"
cloud_stt_provider = "xai"
cloud_llm_provider = "xai"
cloud_llm_model = "grok-4-fast-non-reasoning"
language = "en"
injection_method = "clipboard"
system_prompt = """
You receive a spoken dictation from a SQL reviewer.
Shape it into a clear, technically concise code-review comment:
- One line as the key point.
- Optionally 2-3 lines of rationale with index/plan hints.
- When the speaker names concrete SQL, embed it in `backticks`.
- Language: English, technical, no filler.
- Output ONLY the comment.
"""
```

Save it, done. No app restart needed — hot-reload sees the new
file and immediately adds the mode to the overlay menu.
