// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostics IPC: log stream, app version, system info.

use crate::core::session::SessionInfo;
use crate::core::AppContext;
use crate::injection::{InjectOptions, InjectionStrategy};
use std::sync::Arc;
use std::time::Duration;

type IpcResult<T> = std::result::Result<T, String>;

#[tauri::command]
pub async fn get_app_version() -> IpcResult<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
pub async fn get_recent_logs(
    state: tauri::State<'_, Arc<AppContext>>,
    limit: u32,
) -> IpcResult<Vec<String>> {
    Ok(state.log_buffer.lines(limit as usize))
}

#[tauri::command]
pub async fn get_session_info() -> IpcResult<SessionInfo> {
    Ok(crate::core::session::detect_session())
}

#[tauri::command]
pub async fn get_whisper_backend() -> IpcResult<crate::transcription::backend::WhisperBackendInfo> {
    Ok(crate::transcription::backend::active_backend())
}

#[tauri::command]
pub async fn get_hardware_report() -> IpcResult<crate::core::hardware::HardwareReport> {
    Ok(crate::core::hardware::detect())
}

/// Upper bound for the `test_auto_paste` countdown. Guards the IPC
/// boundary against a malformed/hostile frontend value hanging the
/// inject diagnostic for an arbitrary duration.
const MAX_TEST_AUTO_PASTE_DELAY_SECS: u64 = 30;

fn validate_delay_secs(delay_secs: u64) -> IpcResult<()> {
    if delay_secs > MAX_TEST_AUTO_PASTE_DELAY_SECS {
        return Err(format!(
            "delay_secs must be at most {MAX_TEST_AUTO_PASTE_DELAY_SECS}"
        ));
    }
    Ok(())
}

/// Diagnostic test for auto-paste. Sleeps `delay_secs` seconds so
/// the user has time to focus the target window, then triggers a
/// complete inject (clipboard + libei-Ctrl+V) with `text`. This
/// rules out the focus race of the normal pipeline — if this works
/// but the real dictation path doesn't, libei is not the problem;
/// whatever is moving focus between hotkey press and inject is.
#[tauri::command]
pub async fn test_auto_paste(
    state: tauri::State<'_, Arc<AppContext>>,
    text: String,
    delay_secs: u64,
) -> IpcResult<()> {
    validate_delay_secs(delay_secs)?;
    tracing::info!(
        delay_secs,
        text_len = text.len(),
        "test_auto_paste: countdown started — please focus the target window"
    );
    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
    tracing::info!("test_auto_paste: triggering inject");
    state
        .injector
        .inject(
            &text,
            InjectOptions {
                strategy: InjectionStrategy::Clipboard,
                action: crate::core::OutputAction::Insert,
                paste_with_shift: false,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_at_cap_is_accepted() {
        assert!(validate_delay_secs(MAX_TEST_AUTO_PASTE_DELAY_SECS).is_ok());
    }

    #[test]
    fn delay_above_cap_is_rejected() {
        assert!(validate_delay_secs(MAX_TEST_AUTO_PASTE_DELAY_SECS + 1).is_err());
    }
}
