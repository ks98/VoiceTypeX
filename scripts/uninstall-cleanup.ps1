# SPDX-License-Identifier: GPL-3.0-or-later
#
# VoiceTypeX — complete uninstall cleanup for Windows.
#
# What the NSIS uninstaller does NOT do:
#   - Remove user data under %APPDATA%\<identifier>\ (by design:
#     a re-install should keep modes and settings).
#   - Delete Windows Credential Manager entries under target="voicetypex".
#   - Remove the autostart registry entry, if it was enabled.
#
# This script clears away those leftovers. It performs NO action without
# explicit confirmation; each block prompts individually.
#
# Usage (PowerShell as a normal user, NOT as Admin):
#   powershell -ExecutionPolicy Bypass -File scripts\uninstall-cleanup.ps1
#
# In case of ExecutionPolicy errors:
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
#   .\scripts\uninstall-cleanup.ps1

$ErrorActionPreference = 'Stop'

$Identifier = 'de.kevin-stenzel.voicetypex'
$Service    = 'voicetypex'
$Providers  = @('xai', 'openai', 'anthropic', 'groq', 'deepgram')

# Tauri default on Windows: %APPDATA%\<identifier>\config\, plus
# %APPDATA%\<identifier>\data\ for app_data_dir. We clean up both,
# if present.
$AppData    = [Environment]::GetFolderPath('ApplicationData')
$ConfigDir  = Join-Path $AppData "$Identifier\config"
$DataDir    = Join-Path $AppData "$Identifier\data"
$RootDir    = Join-Path $AppData $Identifier

function Confirm-Action {
    param([string]$Prompt)
    $answer = Read-Host "$Prompt [y/N]"
    return $answer -match '^[yY]'
}

function Write-Header {
    param([string]$Text)
    Write-Host ""
    Write-Host ('=' * 65) -ForegroundColor DarkGray
    Write-Host " $Text" -ForegroundColor Cyan
    Write-Host ('=' * 65) -ForegroundColor DarkGray
}

Write-Host ""
Write-Host "VoiceTypeX Uninstall Cleanup" -ForegroundColor Cyan
Write-Host ""
Write-Host "Configuration:"
Write-Host "  Config dir:       $ConfigDir"
Write-Host "  Data dir:         $DataDir"
Write-Host "  Credential Mgr:   target=$Service"
Write-Host "  Autostart RegKey: HKCU:\Software\Microsoft\Windows\CurrentVersion\Run\VoiceTypeX"
Write-Host ""
Write-Host "Before the first step: make sure VoiceTypeX is NOT running." -ForegroundColor Yellow
Write-Host ""

if (-not (Confirm-Action "Continue with the cleanup?")) {
    Write-Host "Aborted."
    exit 0
}

# --- 1. User data -------------------------------------------------------
Write-Header "Step 1/4 - User data (settings, modes, secrets, Wayland token)"

foreach ($dir in @($ConfigDir, $DataDir, $RootDir)) {
    if (Test-Path $dir) {
        $size = 0
        try {
            $size = (Get-ChildItem $dir -Recurse -ErrorAction SilentlyContinue |
                     Measure-Object -Property Length -Sum).Sum
        } catch {}
        $sizeMb = if ($size) { [math]::Round($size / 1MB, 1) } else { 0 }

        Write-Host ""
        Write-Host "Found: $dir ($sizeMb MB)"
        Write-Host "Contents:"
        Get-ChildItem $dir -ErrorAction SilentlyContinue |
            Select-Object -First 10 |
            ForEach-Object { Write-Host "  $($_.FullName)" }

        if (Confirm-Action "Remove completely?") {
            try {
                Remove-Item -Path $dir -Recurse -Force
                Write-Host "  -> removed." -ForegroundColor Green
            } catch {
                Write-Host "  -> Error: $_" -ForegroundColor Red
            }
        } else {
            Write-Host "  -> skipped."
        }
    }
}

# If $RootDir is empty after the sub-deletions, clean it up too.
if ((Test-Path $RootDir) -and (-not (Get-ChildItem $RootDir -ErrorAction SilentlyContinue))) {
    Remove-Item -Path $RootDir -Force -ErrorAction SilentlyContinue
}

# --- 2. Credential Manager ---------------------------------------------
Write-Header "Step 2/4 - Windows Credential Manager (API keys)"
Write-Host "VoiceTypeX stores API keys best-effort in the Credential Manager"
Write-Host "under target=`"$Service`". Providers: $($Providers -join ', ')"
Write-Host ""

if (Confirm-Action "Delete all $($Providers.Count) provider entries via cmdkey?") {
    foreach ($p in $Providers) {
        # windows-native-keyring-store (backend of keyring-3 on Windows)
        # forms the target_name as "<user>.<service>" with the default
        # delimiter ".", see docs.rs/windows-native-keyring-store. Our calls
        # `keyring::Entry::new("voicetypex", "xai")` thus end up under
        # target="xai.voicetypex".
        #
        # For robustness against a backend change or custom target modifier,
        # after the canonical format we also try the historical spellings —
        # if none match, the entry simply isn't there.
        $canonical = "${p}.${Service}"
        $candidates = @($canonical, "${Service}.${p}", "${Service}:${p}", $p)
        $deleted = $false
        foreach ($target in $candidates) {
            $output = cmdkey /delete:$target 2>&1
            if ($LASTEXITCODE -eq 0) {
                Write-Host "  -> ${p}: deleted (target=$target)" -ForegroundColor Green
                $deleted = $true
                break
            }
        }
        if (-not $deleted) {
            Write-Host "  -> ${p}: no entry found (ok)"
        }
    }
    Write-Host ""
    Write-Host "Complete list of remaining VoiceTypeX entries:"
    cmdkey /list | Select-String -Pattern "$Service" -SimpleMatch |
        ForEach-Object { Write-Host "  $_" }
}

# --- 3. Autostart RegKey ------------------------------------------------
Write-Header "Step 3/4 - Autostart entry (Registry)"
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$AutostartFound = $false

if (Test-Path $RunKey) {
    $entries = Get-ItemProperty -Path $RunKey -ErrorAction SilentlyContinue
    foreach ($name in @('VoiceTypeX', 'voicetypex', $Identifier)) {
        if ($entries.PSObject.Properties.Name -contains $name) {
            $AutostartFound = $true
            Write-Host "  Found: $name = $($entries.$name)"
            if (Confirm-Action "Remove?") {
                Remove-ItemProperty -Path $RunKey -Name $name
                Write-Host "    -> removed." -ForegroundColor Green
            }
        }
    }
}
if (-not $AutostartFound) {
    Write-Host "  No autostart entry found - nothing to do."
}

# --- 4. Manual notes ---------------------------------------------------
Write-Header "Step 4/4 - Manual steps"
Write-Host ""
Write-Host "This script CANNOT remove the following leftovers:"
Write-Host ""
Write-Host "  - WebView2 profile cache (browser state of the app webviews):"
Write-Host "    %LocalAppData%\$Identifier\EBWebView\"
Write-Host "    -> delete manually, if desired"
Write-Host ""
Write-Host "  - NSIS uninstaller entry in Programs/Features:"
Write-Host "    -> Win+R 'appwiz.cpl' -> VoiceTypeX -> Uninstall"
Write-Host ""
Write-Host "  - Start Menu entry (removed by the NSIS uninstaller):"
Write-Host "    %APPDATA%\Microsoft\Windows\Start Menu\Programs\VoiceTypeX"
Write-Host ""

Write-Header "Done"
Write-Host ""
Write-Host "VoiceTypeX leftovers on this system have been removed as thoroughly as possible." -ForegroundColor Green
Write-Host "For questions or problems: docs\PLATFORMS.md -> Uninstallation."
Write-Host ""
