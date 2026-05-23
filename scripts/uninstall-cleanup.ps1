# SPDX-License-Identifier: GPL-3.0-or-later
#
# VoiceTypeX — vollständiger Uninstall-Cleanup für Windows.
#
# Was der NSIS-Uninstaller NICHT macht:
#   - User-Daten unter %APPDATA%\<identifier>\ entfernen (Absicht:
#     Re-Install soll Modi und Settings behalten).
#   - Windows-Credential-Manager-Einträge unter target="voicetypex"
#     löschen.
#   - Autostart-Registry-Eintrag entfernen, falls aktiviert war.
#
# Dieses Skript räumt diese Spuren weg. Es macht KEINE Aktion ohne
# explizite Bestätigung; jeder Block fragt einzeln nach.
#
# Aufruf (PowerShell als normaler User, NICHT als Admin):
#   powershell -ExecutionPolicy Bypass -File scripts\uninstall-cleanup.ps1
#
# Falls ExecutionPolicy-Fehler:
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
#   .\scripts\uninstall-cleanup.ps1

$ErrorActionPreference = 'Stop'

$Identifier = 'de.kevin-stenzel.voicetypex'
$Service    = 'voicetypex'
$Providers  = @('xai', 'openai', 'anthropic', 'groq', 'deepgram')

# Tauri-Default auf Windows: %APPDATA%\<identifier>\config\, plus
# %APPDATA%\<identifier>\data\ fuer app_data_dir. Wir raeumen beides,
# falls vorhanden.
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
Write-Host "VoiceTypeX Uninstall-Cleanup" -ForegroundColor Cyan
Write-Host ""
Write-Host "Konfiguration:"
Write-Host "  Config-Dir:       $ConfigDir"
Write-Host "  Data-Dir:         $DataDir"
Write-Host "  Credential-Mgr:   target=$Service"
Write-Host "  Autostart-RegKey: HKCU:\Software\Microsoft\Windows\CurrentVersion\Run\VoiceTypeX"
Write-Host ""
Write-Host "Vor dem ersten Schritt: stelle sicher, dass VoiceTypeX NICHT laeuft." -ForegroundColor Yellow
Write-Host ""

if (-not (Confirm-Action "Mit dem Cleanup fortfahren?")) {
    Write-Host "Abgebrochen."
    exit 0
}

# --- 1. User-Daten ------------------------------------------------------
Write-Header "Schritt 1/4 - User-Daten (Settings, Modi, Secrets, Wayland-Token)"

foreach ($dir in @($ConfigDir, $DataDir, $RootDir)) {
    if (Test-Path $dir) {
        $size = 0
        try {
            $size = (Get-ChildItem $dir -Recurse -ErrorAction SilentlyContinue |
                     Measure-Object -Property Length -Sum).Sum
        } catch {}
        $sizeMb = if ($size) { [math]::Round($size / 1MB, 1) } else { 0 }

        Write-Host ""
        Write-Host "Gefunden: $dir ($sizeMb MB)"
        Write-Host "Inhalt:"
        Get-ChildItem $dir -ErrorAction SilentlyContinue |
            Select-Object -First 10 |
            ForEach-Object { Write-Host "  $($_.FullName)" }

        if (Confirm-Action "Komplett entfernen?") {
            try {
                Remove-Item -Path $dir -Recurse -Force
                Write-Host "  -> entfernt." -ForegroundColor Green
            } catch {
                Write-Host "  -> Fehler: $_" -ForegroundColor Red
            }
        } else {
            Write-Host "  -> uebersprungen."
        }
    }
}

# Falls $RootDir nach den Sub-Loeschungen leer ist, auch wegraeumen.
if ((Test-Path $RootDir) -and (-not (Get-ChildItem $RootDir -ErrorAction SilentlyContinue))) {
    Remove-Item -Path $RootDir -Force -ErrorAction SilentlyContinue
}

# --- 2. Credential-Manager ---------------------------------------------
Write-Header "Schritt 2/4 - Windows Credential Manager (API-Keys)"
Write-Host "VoiceTypeX speichert API-Keys best-effort im Credential-Manager"
Write-Host "unter target=`"$Service`". Provider: $($Providers -join ', ')"
Write-Host ""

if (Confirm-Action "Mit cmdkey alle $($Providers.Count) Provider-Eintraege loeschen?") {
    foreach ($p in $Providers) {
        # windows-native-keyring-store (Backend von keyring-3 auf Windows)
        # bildet target_name als "<user>.<service>" mit Default-Delimiter
        # ".", siehe docs.rs/windows-native-keyring-store. Unsere Calls
        # `keyring::Entry::new("voicetypex", "xai")` landen damit unter
        # target="xai.voicetypex".
        #
        # Aus Robustheit gegen Backend-Wechsel oder Custom-target-Modifier
        # probieren wir nach dem kanonischen Format noch die historischen
        # Schreibweisen — wenn nichts greift, ist der Eintrag schlicht
        # nicht da.
        $canonical = "${p}.${Service}"
        $candidates = @($canonical, "${Service}.${p}", "${Service}:${p}", $p)
        $deleted = $false
        foreach ($target in $candidates) {
            $output = cmdkey /delete:$target 2>&1
            if ($LASTEXITCODE -eq 0) {
                Write-Host "  -> ${p}: geloescht (target=$target)" -ForegroundColor Green
                $deleted = $true
                break
            }
        }
        if (-not $deleted) {
            Write-Host "  -> ${p}: kein Eintrag gefunden (ok)"
        }
    }
    Write-Host ""
    Write-Host "Vollstaendige Liste verbleibender VoiceTypeX-Eintraege:"
    cmdkey /list | Select-String -Pattern "$Service" -SimpleMatch |
        ForEach-Object { Write-Host "  $_" }
}

# --- 3. Autostart-RegKey ------------------------------------------------
Write-Header "Schritt 3/4 - Autostart-Eintrag (Registry)"
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$AutostartFound = $false

if (Test-Path $RunKey) {
    $entries = Get-ItemProperty -Path $RunKey -ErrorAction SilentlyContinue
    foreach ($name in @('VoiceTypeX', 'voicetypex', $Identifier)) {
        if ($entries.PSObject.Properties.Name -contains $name) {
            $AutostartFound = $true
            Write-Host "  Gefunden: $name = $($entries.$name)"
            if (Confirm-Action "Entfernen?") {
                Remove-ItemProperty -Path $RunKey -Name $name
                Write-Host "    -> entfernt." -ForegroundColor Green
            }
        }
    }
}
if (-not $AutostartFound) {
    Write-Host "  Kein Autostart-Eintrag gefunden - nichts zu tun."
}

# --- 4. Manuelle Hinweise ----------------------------------------------
Write-Header "Schritt 4/4 - Manuelle Schritte"
Write-Host ""
Write-Host "Folgende Spuren kann dieses Skript NICHT entfernen:"
Write-Host ""
Write-Host "  - WebView2-Profil-Cache (Browser-State der App-Webviews):"
Write-Host "    %LocalAppData%\$Identifier\EBWebView\"
Write-Host "    -> manuell loeschen, falls gewuenscht"
Write-Host ""
Write-Host "  - NSIS-Uninstaller-Eintrag in Programme/Features:"
Write-Host "    -> Win+R 'appwiz.cpl' -> VoiceTypeX -> Deinstallieren"
Write-Host ""
Write-Host "  - Start-Menu-Eintrag (wird vom NSIS-Uninstaller entfernt):"
Write-Host "    %APPDATA%\Microsoft\Windows\Start Menu\Programs\VoiceTypeX"
Write-Host ""

Write-Header "Fertig"
Write-Host ""
Write-Host "VoiceTypeX-Spuren auf diesem System wurden bestmoeglich entfernt." -ForegroundColor Green
Write-Host "Bei Fragen oder Problemen: docs\PLATFORMS.md -> Deinstallation."
Write-Host ""
