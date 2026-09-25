// SPDX-License-Identifier: GPL-3.0-or-later
//! ChatGPT account IPC (experimental): sign in, cancel, paste fallback,
//! sign out. Tokens never leave the backend — the frontend only sees
//! [`ChatGptStatus`], pushed on every change via `events::CHATGPT_STATUS`.

use crate::chatgpt::{loopback, oauth, ChatGptStatus};
use crate::core::app_context::AppContext;
use crate::core::events;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;

type IpcResult<T> = std::result::Result<T, String>;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn emit_status(app: &AppHandle, status: &ChatGptStatus) {
    if let Err(e) = app.emit(events::CHATGPT_STATUS, status) {
        tracing::warn!(error = %e, "emit chatgpt status failed");
    }
}

#[tauri::command]
pub async fn get_chatgpt_status(
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<ChatGptStatus> {
    Ok(state.chatgpt.status())
}

/// Starts a sign-in: binds the loopback callback, opens the browser and
/// returns immediately with the pending status. Completion (or failure /
/// timeout) arrives as a status event.
#[tauri::command]
pub async fn chatgpt_login_start(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<ChatGptStatus> {
    let ctx = Arc::clone(&state);
    // Free the port of a previous attempt before binding again.
    if let Some(previous) = ctx.chatgpt.cancel_login() {
        let _ = previous.await;
    }

    let listener = loopback::bind().await;
    let port = listener.as_ref().map(|(_, p)| *p).unwrap_or(1455);
    let (auth_url, login_state) = ctx.chatgpt.begin_login(oauth::redirect_uri(port));

    if let Some((listener, _)) = listener {
        let task_ctx = Arc::clone(&ctx);
        let task_app = app.clone();
        let task_state = login_state.clone();
        let task = tauri::async_runtime::spawn(async move {
            let session = &task_ctx.chatgpt;
            match tokio::time::timeout(LOGIN_TIMEOUT, loopback::wait_for_callback(listener)).await {
                Ok(Ok(target)) => {
                    session.detach_task(&task_state);
                    if let Err(e) = session.complete_login(&task_ctx.http_client, &target).await {
                        session.fail_login(&task_state, e.to_string());
                    }
                }
                Ok(Err(e)) => session.fail_login(
                    &task_state,
                    format!("ChatGPT sign-in: callback listener failed: {e}"),
                ),
                Err(_) => session.fail_login(&task_state, "ChatGPT sign-in timed out".into()),
            }
            emit_status(&task_app, &session.status());
        });
        ctx.chatgpt.attach_task(&login_state, task);
    }

    if let Err(e) = app.opener().open_url(&auth_url, None::<&str>) {
        // The UI still offers "copy sign-in link" from the pending status.
        tracing::warn!(error = %e, "could not open the browser for ChatGPT sign-in");
    }

    let status = ctx.chatgpt.status();
    emit_status(&app, &status);
    Ok(status)
}

/// Fallback when the browser could not reach the loopback callback: the
/// user pastes the address the browser ended up on.
#[tauri::command]
pub async fn chatgpt_login_complete_manual(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
    redirect_url: String,
) -> IpcResult<ChatGptStatus> {
    state
        .chatgpt
        .complete_login(&state.http_client, &redirect_url)
        .await
        .map_err(|e| e.to_string())?;
    let status = state.chatgpt.status();
    emit_status(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn chatgpt_login_cancel(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<()> {
    if let Some(task) = state.chatgpt.cancel_login() {
        let _ = task.await;
    }
    emit_status(&app, &state.chatgpt.status());
    Ok(())
}

#[tauri::command]
pub async fn chatgpt_logout(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppContext>>,
) -> IpcResult<()> {
    state.chatgpt.logout().map_err(|e| e.to_string())?;
    emit_status(&app, &state.chatgpt.status());
    Ok(())
}
