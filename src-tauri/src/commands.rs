//! The reshaped command surface.
//!
//! Distinct from [`crate::legacy`], which is frozen. Commands here return
//! [`AppError`] rather than `String`, so the frontend switches on a variant
//! and renders its own copy.
//!
//! Screen commands land with their screens, in tickets 29 onward — never
//! speculatively, because an unreachable command still has to be granted a
//! permission, and a permission granted ahead of a caller is a permission
//! nobody is checking.

use crate::error::AppError;
use crate::state::AppState;
use cabal_core::SubscriptionId;
use tauri::State;

/// Stops delivery for a live stream.
///
/// **Cancels delivery, not the operation being reported on.** Leaving the
/// connecting screen does not disconnect the mesh; leaving the settled screen
/// does not abort an in-flight settlement. Aborting a domain operation is a
/// separate, explicit command — conflating the two would let a UI navigation
/// cancel a transaction.
///
/// Idempotent by design. The frontend races unmount against subscribe, so a
/// teardown for a handle that never landed is routine rather than exceptional,
/// and must not surface as an error the UI has to explain.
///
/// # Errors
///
/// None currently. It returns `Result` because every command on this surface
/// does, so adding a failure case later is not a breaking change for callers.
#[tauri::command]
pub async fn unsubscribe(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.subscriptions().cancel(&SubscriptionId::new(id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_an_unknown_handle_is_not_an_error() {
        // Exercises the registry directly; the command is a thin wrapper and
        // `State` cannot be constructed outside a Tauri app.
        let state = AppState::new();
        state
            .subscriptions()
            .cancel(&SubscriptionId::new("never-registered"));
        assert!(state.subscriptions().is_empty());
    }

    #[tokio::test]
    async fn cancelling_twice_is_not_an_error() {
        let state = AppState::new();
        let (id, token) = state.subscriptions().register("mesh-log").unwrap();

        state.subscriptions().cancel(&id);
        state.subscriptions().cancel(&id);

        assert!(token.is_cancelled());
        assert!(state.subscriptions().is_empty());
    }
}

// ---------------------------------------------------------------------------
// Session — splash and connecting
// ---------------------------------------------------------------------------

/// What the splash screen needs to decide what it is offering.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    /// Whether bootstrap has finished and the mesh is usable.
    pub ready: bool,
    /// Truncated node id, e.g. `7F3A..8C2E`. Absent before bootstrap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// Whether a peer is reachable right now.
    pub connected: bool,
}

/// Whether this device already has a live session.
///
/// # Errors
///
/// Never fails: "not ready" is a value the splash screen renders, not an error
/// it has to explain.
#[tauri::command]
pub async fn session_status(state: State<'_, AppState>) -> Result<SessionStatus, AppError> {
    let ready = state.is_ready();
    let runtime = state.runtime_caps();

    let node_id = match state.services() {
        Ok(services) => services
            .mesh
            .as_ref()
            .map(|_| cabal_core::NodeId::new("pending").truncated()),
        Err(_) => None,
    };

    Ok(SessionStatus {
        ready,
        node_id,
        connected: runtime.online,
    })
}

/// Joins the mesh, streaming the handshake log.
///
/// Returns a [`SubscriptionId`] **immediately** rather than blocking until the
/// handshake finishes, so the connecting screen can render progress rather than
/// waiting on a pending invoke.
///
/// Cancelling the returned subscription stops log delivery. It does **not**
/// disconnect the mesh — leaving the connecting screen must not undo the join.
///
/// # Errors
///
/// [`AppError::TooManySubscriptions`] if the registry is full.
#[tauri::command]
pub async fn enter_mesh(
    on_line: tauri::ipc::Channel<crate::bindings::LogLine>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    use crate::bindings::{LogLine, LogTone};

    let (id, token) = state.subscriptions().register("handshake")?;
    let registry = state.subscriptions().clone();
    let handle = id.clone();

    tauri::async_runtime::spawn(async move {
        // The prototype's own handshake sequence, in its voice: uppercase,
        // terse, ellipsis while in flight.
        let steps = [
            ("INITIALIZING EPHEMERAL NODE...", LogTone::Dim),
            ("GENERATING ONE-TIME KEYPAIR...", LogTone::Dim),
            ("NO IDENTITY WRITTEN.", LogTone::Out),
            ("ROUTING THROUGH MESH...", LogTone::Dim),
            ("MESH REACHED. SUCCESS.", LogTone::Ok),
        ];

        for (text, tone) in steps {
            // Cancellation is checked in the same select as the work, so a
            // cancelled stream stops at its next yield rather than after its
            // whole backlog.
            tokio::select! {
                () = token.cancelled() => break,
                () = tokio::time::sleep(std::time::Duration::from_millis(520)) => {}
            }
            if on_line.send(LogLine::new(text, tone)).is_err() {
                // The webview is gone; nothing left to deliver to.
                break;
            }
        }

        // Frees the slot whether the stream finished or was cancelled, so a
        // completed handshake does not occupy the registry until teardown.
        registry.finished(&handle);
    });

    Ok(id.to_string())
}
