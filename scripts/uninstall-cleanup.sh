#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# VoiceTypeX — full uninstall cleanup for Linux/macOS.
#
# What the OS package manager does NOT do:
#   - Remove user data under ~/.config/<identifier>/ (by design: a re-
#     install should keep your modes and settings).
#   - Delete OS keychain entries under service="voicetypex".
#   - Remove the autostart entry, if the user had enabled autostart.
#
# This script clears those leftovers. It performs NO action without
# explicit confirmation; each block asks individually.
#
# Usage:
#   bash scripts/uninstall-cleanup.sh
#
# Requirements: bash (>=4), optionally `secret-tool` (libsecret-tools)
# for the keyring deletion. Without secret-tool you get a warning with
# instructions on how to do it manually in seahorse/kwallet.

set -u

IDENTIFIER="de.kevin-stenzel.voicetypex"
SERVICE="voicetypex"
PROVIDERS=("xai" "openai" "anthropic" "groq" "deepgram")

# Platform detection — determines the paths.
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
        echo "Platform $(uname -s) is not supported by this script."
        echo "For Windows, use scripts/uninstall-cleanup.ps1."
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
echo "VoiceTypeX Uninstall Cleanup"
echo ""
echo "Configuration:"
echo "  Config dir: $CONFIG_DIR"
echo "  Data dir:   $DATA_DIR"
echo "  Autostart:  $AUTOSTART_DIR"
echo "  Keychain service: $SERVICE"
echo ""
echo "Before the first step: make sure VoiceTypeX is NOT running."
echo ""
if ! confirm "Continue with the cleanup?"; then
    echo "Aborted."
    exit 0
fi

# ─── 1. User data (config + data) ──────────────────────────────────────
print_header "Step 1/4 — User data (settings, modes, secrets, Wayland token)"
if [[ -d "$CONFIG_DIR" ]]; then
    echo "The following contents will be deleted:"
    find "$CONFIG_DIR" -maxdepth 2 -mindepth 1 -printf "  %p\n" 2>/dev/null | head -20
    SIZE=$(du -sh "$CONFIG_DIR" 2>/dev/null | cut -f1)
    echo "  (Total size: $SIZE)"
    if confirm "Remove the config dir completely?"; then
        rm -rf "$CONFIG_DIR"
        echo "  → Config dir removed."
    else
        echo "  → skipped."
    fi
else
    echo "  (Config dir does not exist — nothing to do.)"
fi

# Models live under app_config_dir for VoiceTypeX, NOT under app_data_dir.
# DATA_DIR is only a precaution — in case Tauri plugins write there
# after all.
if [[ "$DATA_DIR" != "$CONFIG_DIR" && -d "$DATA_DIR" ]]; then
    SIZE=$(du -sh "$DATA_DIR" 2>/dev/null | cut -f1)
    echo ""
    echo "Additionally found: $DATA_DIR ($SIZE)"
    if confirm "Remove the data dir as well?"; then
        rm -rf "$DATA_DIR"
        echo "  → Data dir removed."
    fi
fi

# ─── 2. OS keychain ────────────────────────────────────────────────────
print_header "Step 2/4 — OS keychain entries (API keys)"
echo "VoiceTypeX stores API keys best-effort additionally in the OS keychain"
echo "under service=\"$SERVICE\". Providers: ${PROVIDERS[*]}"
echo ""

if [[ $IS_MAC -eq 1 ]]; then
    # macOS: security command
    if confirm "Delete all ${#PROVIDERS[@]} provider entries with /usr/bin/security?"; then
        for p in "${PROVIDERS[@]}"; do
            if security delete-generic-password -s "$SERVICE" -a "$p" 2>/dev/null; then
                echo "  → $p: deleted"
            else
                echo "  → $p: not found (ok)"
            fi
        done
    fi
elif command -v secret-tool >/dev/null 2>&1; then
    # Linux with libsecret/secret-tool
    if confirm "Delete all ${#PROVIDERS[@]} provider entries from libsecret/gnome-keyring with secret-tool?"; then
        for p in "${PROVIDERS[@]}"; do
            if secret-tool clear service "$SERVICE" username "$p" 2>/dev/null; then
                echo "  → $p: deleted"
            else
                echo "  → $p: not found or backend error (ok)"
            fi
        done
    fi
else
    echo "  secret-tool is not installed (package: libsecret-tools)."
    echo ""
    echo "  Remove manually:"
    echo "    • GNOME Keyring: open seahorse → search \"$SERVICE\" → delete"
    echo "    • KWallet:       kwalletmanager5/6 → \"Passwords\" → \"$SERVICE\" → delete"
    echo ""
    echo "  Or install libsecret-tools and run this script again:"
    echo "    sudo apt-get install libsecret-tools"
fi

# Additionally KWallet, if installed separately (KDE setups)
if command -v kwallet-query >/dev/null 2>&1; then
    echo ""
    echo "  KWallet detected — in case keys live there (instead of in libsecret):"
    echo "    kwallet-query -l kdewallet | grep -i $SERVICE"
    echo "  and delete the entries in the kwalletmanager GUI."
fi

# ─── 3. Autostart entry ────────────────────────────────────────────────
print_header "Step 3/4 — Autostart entry"
AUTOSTART_FOUND=0
if [[ -d "$AUTOSTART_DIR" ]]; then
    # tauri-plugin-autostart typically creates a .desktop file named after
    # the app identifier or ProductName. We look for both variants.
    for pattern in "$IDENTIFIER.desktop" "VoiceTypeX.desktop" "voicetypex.desktop" "*VoiceType*.desktop"; do
        for match in "$AUTOSTART_DIR"/$pattern; do
            if [[ -f "$match" ]]; then
                AUTOSTART_FOUND=1
                echo "  Found: $match"
                if confirm "Remove?"; then
                    rm -f "$match"
                    echo "    → removed."
                fi
            fi
        done
    done
fi
if [[ $AUTOSTART_FOUND -eq 0 ]]; then
    echo "  No autostart entry found — nothing to do."
fi

# ─── 4. Notes on leftovers that cannot be cleaned ──────────────────────
print_header "Step 4/4 — Manual steps"
echo ""
echo "This script CANNOT remove the following leftovers:"
echo ""
echo "  • Wayland portal permission for auto-paste (xdg-desktop-portal)"
echo "    → KDE Plasma 6: System Settings → Apps → Application Permissions"
echo "                    → \"Send keystrokes\" / \"RemoteDesktop\""
echo "                    → remove VoiceTypeX"
echo "    → GNOME:        gsettings list-recursively | grep desktop-portal"
echo "                    + cleanup via dconf-editor"
echo ""
echo "  • KDE global shortcut (hotkey assignment)"
echo "    → System Settings → Global Shortcuts → VoiceTypeX → Reset"
echo ""
echo "  • OS package manager entry itself (if not already removed)"
echo "    → Debian/Ubuntu:  sudo apt remove voice-type-x"
echo "    → Fedora/RHEL:    sudo dnf remove voice-type-x"
echo "    → AppImage:       simply delete the AppImage file"
echo ""

print_header "Done"
echo ""
echo "VoiceTypeX leftovers on this system have been removed as best as possible."
echo "For questions or problems: docs/PLATFORMS.md → Uninstallation."
echo ""
