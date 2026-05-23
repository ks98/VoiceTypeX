#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# VoiceTypeX — vollständiger Uninstall-Cleanup für Linux/macOS.
#
# Was der OS-Paket-Manager NICHT macht:
#   - User-Daten unter ~/.config/<identifier>/ entfernen (Absicht: Re-
#     install soll Modi und Settings behalten).
#   - OS-Keychain-Einträge unter service="voicetypex" löschen.
#   - Autostart-Eintrag entfernen, falls der User Autostart aktiviert hatte.
#
# Dieses Skript räumt diese Spuren weg. Es macht KEINE Aktion ohne
# explizite Bestätigung; jeder Block fragt einzeln nach.
#
# Aufruf:
#   bash scripts/uninstall-cleanup.sh
#
# Voraussetzungen: bash (>=4), optional `secret-tool` (libsecret-tools)
# für die Keyring-Löschung. Ohne secret-tool gibt's eine Warnung mit
# Anleitung, wie der User es manuell in seahorse/kwallet macht.

set -u

IDENTIFIER="de.kevin-stenzel.voicetypex"
SERVICE="voicetypex"
PROVIDERS=("xai" "openai" "anthropic" "groq" "deepgram")

# Plattform-Erkennung — bestimmt die Pfade.
case "$(uname -s)" in
    Linux*)
        CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/$IDENTIFIER"
        DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/$IDENTIFIER"
        AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
        IS_MAC=0
        ;;
    Darwin*)
        CONFIG_DIR="$HOME/Library/Application Support/$IDENTIFIER"
        DATA_DIR="$HOME/Library/Application Support/$IDENTIFIER"
        AUTOSTART_DIR="$HOME/Library/LaunchAgents"
        IS_MAC=1
        ;;
    *)
        echo "Plattform $(uname -s) wird von diesem Skript nicht unterstützt."
        echo "Für Windows nutze scripts/uninstall-cleanup.ps1."
        exit 1
        ;;
esac

confirm() {
    local prompt="$1"
    local answer
    read -r -p "$prompt [y/N] " answer
    case "$answer" in
        y|Y|yes|YES) return 0 ;;
        *) return 1 ;;
    esac
}

print_header() {
    echo ""
    echo "═══════════════════════════════════════════════════════════════"
    echo " $1"
    echo "═══════════════════════════════════════════════════════════════"
}

echo ""
echo "VoiceTypeX Uninstall-Cleanup"
echo ""
echo "Konfiguration:"
echo "  Config-Dir: $CONFIG_DIR"
echo "  Data-Dir:   $DATA_DIR"
echo "  Autostart:  $AUTOSTART_DIR"
echo "  Keychain-Service: $SERVICE"
echo ""
echo "Vor dem ersten Schritt: stelle sicher, dass VoiceTypeX NICHT läuft."
echo ""
if ! confirm "Mit dem Cleanup fortfahren?"; then
    echo "Abgebrochen."
    exit 0
fi

# ─── 1. User-Daten (Config + Daten) ────────────────────────────────────
print_header "Schritt 1/4 — User-Daten (Settings, Modi, Secrets, Wayland-Token)"
if [[ -d "$CONFIG_DIR" ]]; then
    echo "Folgende Inhalte werden gelöscht:"
    find "$CONFIG_DIR" -maxdepth 2 -mindepth 1 -printf "  %p\n" 2>/dev/null | head -20
    SIZE=$(du -sh "$CONFIG_DIR" 2>/dev/null | cut -f1)
    echo "  (Gesamtgröße: $SIZE)"
    if confirm "Config-Dir komplett entfernen?"; then
        rm -rf "$CONFIG_DIR"
        echo "  → Config-Dir entfernt."
    else
        echo "  → übersprungen."
    fi
else
    echo "  (Config-Dir existiert nicht — nichts zu tun.)"
fi

# Models liegen unter app_config_dir bei VoiceTypeX, NICHT unter app_data_dir.
# DATA_DIR ist nur als Vorsichtsmaßnahme — falls Tauri-Plugins doch
# dorthin schreiben.
if [[ "$DATA_DIR" != "$CONFIG_DIR" && -d "$DATA_DIR" ]]; then
    SIZE=$(du -sh "$DATA_DIR" 2>/dev/null | cut -f1)
    echo ""
    echo "Zusätzlich gefunden: $DATA_DIR ($SIZE)"
    if confirm "Data-Dir ebenfalls entfernen?"; then
        rm -rf "$DATA_DIR"
        echo "  → Data-Dir entfernt."
    fi
fi

# ─── 2. OS-Keychain ────────────────────────────────────────────────────
print_header "Schritt 2/4 — OS-Keychain-Einträge (API-Keys)"
echo "VoiceTypeX speichert API-Keys best-effort zusätzlich im OS-Keychain"
echo "unter service=\"$SERVICE\". Provider: ${PROVIDERS[*]}"
echo ""

if [[ $IS_MAC -eq 1 ]]; then
    # macOS: security-Command
    if confirm "Mit /usr/bin/security alle ${#PROVIDERS[@]} Provider-Einträge löschen?"; then
        for p in "${PROVIDERS[@]}"; do
            if security delete-generic-password -s "$SERVICE" -a "$p" 2>/dev/null; then
                echo "  → $p: gelöscht"
            else
                echo "  → $p: nicht gefunden (ok)"
            fi
        done
    fi
elif command -v secret-tool >/dev/null 2>&1; then
    # Linux mit libsecret/secret-tool
    if confirm "Mit secret-tool alle ${#PROVIDERS[@]} Provider-Einträge aus libsecret/gnome-keyring löschen?"; then
        for p in "${PROVIDERS[@]}"; do
            if secret-tool clear service "$SERVICE" username "$p" 2>/dev/null; then
                echo "  → $p: gelöscht"
            else
                echo "  → $p: nicht gefunden oder Backend-Fehler (ok)"
            fi
        done
    fi
else
    echo "  secret-tool nicht installiert (Paket: libsecret-tools)."
    echo ""
    echo "  Manuell entfernen:"
    echo "    • GNOME-Keyring: seahorse öffnen → Suche \"$SERVICE\" → löschen"
    echo "    • KWallet:       kwalletmanager5/6 → \"Passwörter\" → \"$SERVICE\" → löschen"
    echo ""
    echo "  Oder libsecret-tools installieren und dieses Skript erneut ausführen:"
    echo "    sudo apt-get install libsecret-tools"
fi

# Zusätzlich KWallet, falls separat installiert (KDE-Setups)
if command -v kwallet-query >/dev/null 2>&1; then
    echo ""
    echo "  KWallet erkannt — falls Keys dort liegen (statt in libsecret):"
    echo "    kwallet-query -l kdewallet | grep -i $SERVICE"
    echo "  und Einträge in kwalletmanager-gui löschen."
fi

# ─── 3. Autostart-Eintrag ──────────────────────────────────────────────
print_header "Schritt 3/4 — Autostart-Eintrag"
AUTOSTART_FOUND=0
if [[ -d "$AUTOSTART_DIR" ]]; then
    # tauri-plugin-autostart legt typischerweise einen .desktop-File mit
    # dem App-Identifier oder ProductName an. Wir suchen beide Varianten.
    for pattern in "$IDENTIFIER.desktop" "VoiceTypeX.desktop" "voicetypex.desktop" "*VoiceType*.desktop"; do
        for match in "$AUTOSTART_DIR"/$pattern; do
            if [[ -f "$match" ]]; then
                AUTOSTART_FOUND=1
                echo "  Gefunden: $match"
                if confirm "Entfernen?"; then
                    rm -f "$match"
                    echo "    → entfernt."
                fi
            fi
        done
    done
fi
if [[ $AUTOSTART_FOUND -eq 0 ]]; then
    echo "  Kein Autostart-Eintrag gefunden — nichts zu tun."
fi

# ─── 4. Hinweise zu nicht-räumbaren Spuren ─────────────────────────────
print_header "Schritt 4/4 — Manuelle Schritte"
echo ""
echo "Folgende Spuren kann dieses Skript NICHT entfernen:"
echo ""
echo "  • Wayland-Portal-Permission für Auto-Paste (xdg-desktop-portal)"
echo "    → KDE Plasma 6: System-Settings → Apps → Anwendungsberechtigungen"
echo "                    → \"Tastendrücke senden\" / \"RemoteDesktop\""
echo "                    → VoiceTypeX entfernen"
echo "    → GNOME:        gsettings list-recursively | grep desktop-portal"
echo "                    + Cleanup via dconf-editor"
echo ""
echo "  • KDE-Globale-Verknüpfung (Hotkey-Zuweisung)"
echo "    → System-Settings → Globale Verknüpfungen → VoiceTypeX → Reset"
echo ""
echo "  • OS-Paket-Manager-Eintrag selbst (falls noch nicht entfernt)"
echo "    → Debian/Ubuntu:  sudo apt remove voice-type-x"
echo "    → Fedora/RHEL:    sudo dnf remove voice-type-x"
echo "    → AppImage:       AppImage-Datei einfach löschen"
echo ""

print_header "Fertig"
echo ""
echo "VoiceTypeX-Spuren auf diesem System wurden bestmöglich entfernt."
echo "Bei Fragen oder Problemen: docs/PLATFORMS.md → Deinstallation."
echo ""
