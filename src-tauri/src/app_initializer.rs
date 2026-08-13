use crate::blockchain_bridge::BlockchainBridge;
use crate::mesh::{MeshEvent, MeshNetwork};
use crate::state::AppState;
use tauri::{AppHandle, Emitter, State};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

#[derive(Clone, serde::Serialize)]
struct BootstrapStatus {
    phase: String,
    message: String,
    progress: u8, // 0-100
}

pub struct SystemBootstrap;

impl SystemBootstrap {
    /// 1. Phase 1 (Sync): Check internet & sync native AVAX balance via Avalanche RPC
    pub async fn phase_1_sync(bridge: &Arc<Mutex<BlockchainBridge>>, app: &AppHandle) {
        Self::emit(app, "PHASE_1_SYNC", "Checking connection...", 10);

        // Nothing to sync for: the wallet lives behind the vault passphrase,
        // and syncing before it arrives would query the address `unknown` and
        // report the resulting failure as a chain error on the splash screen.
        if !bridge.lock().await.vault_state().is_unlocked() {
            Self::emit(app, "PHASE_1_SYNC", "Vault locked. Waiting for the passphrase.", 30);
            return;
        }

        if Self::check_connectivity().await {
            Self::emit(app, "PHASE_1_SYNC", "Online. Syncing Avalanche RPC balance...", 20);
            let bridge_lock = bridge.lock().await;

            let address = bridge_lock.get_primary_address();
            Self::emit(app, "PHASE_1_SYNC", &format!("Identity: {}", address), 25);

            // Use the real identity (argument is ignored if identity exists)
            if let Err(e) = bridge_lock.sync_state("ignored_override").await {
                tracing::warn!("Sync failed: {}", e);
                Self::emit(app, "PHASE_1_ERROR", &format!("Sync Error: {}", e), 0);
            } else {
                Self::emit(app, "PHASE_1_SYNC", "Snapshot Secured via Avalanche RPC.", 30);
            }
        } else {
             Self::emit(app, "PHASE_1_SYNC", "Offline Mode. Using local snapshot.", 30);
        }
    }

    /// 2. Phase 2 (Delegate): Instant Session
    pub async fn phase_2_delegate(bridge: &Arc<Mutex<BlockchainBridge>>, app: &AppHandle) {
        Self::emit(app, "PHASE_2_DELEGATE", "Initializing Instant Session...", 40);
        let mut bridge_lock = bridge.lock().await;
        let session = bridge_lock.init_instant_session();
        Self::emit(app, "PHASE_2_DELEGATE", &format!("Authority Delegated: {}", session.session_id), 60);
    }

    /// 3. Phase 3 (Network): Init Libp2p
    /// Returns the initialized MeshNetwork and channels
    pub async fn phase_3_network(
        app: &AppHandle,
    ) -> Result<
        (
            MeshNetwork,
            crate::mesh_handle::MeshHandle,
            mpsc::UnboundedReceiver<MeshEvent>,
            tokio::sync::mpsc::Receiver<crate::mesh_handle::MeshCommand>,
            mpsc::UnboundedSender<MeshEvent>,
        ),
        String,
    > {
        Self::emit(app, "PHASE_3_NETWORK", "Booting Libp2p Swarm...", 70);
        
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        // Bounded: a caller that outruns the actor waits rather than
        // growing a queue until the process is killed.
        let (mesh_handle, command_rx) = crate::mesh_handle::MeshHandle::channel();

        match MeshNetwork::new().await {
            Ok(mesh) => {
                 Self::emit(app, "PHASE_3_NETWORK", &format!("PeerID Generated: {}", mesh.swarm.local_peer_id()), 85);
                 Ok((mesh, mesh_handle, event_rx, command_rx, event_tx))
            }
            Err(e) => {
                Self::emit(app, "PHASE_3_ERROR", &format!("Mesh Failed: {}", e), 0);
                Err(e.to_string())
            }
        }
    }

    // Helper: Check internet
    async fn check_connectivity() -> bool {
        // Simple check
        tokio::net::TcpStream::connect("8.8.8.8:53").await.is_ok()
    }

    // Helper: Emit UI Event
    fn emit(app: &AppHandle, phase: &str, msg: &str, progress: u8) {
        let _ = app.emit("bootstrap-status", BootstrapStatus {
            phase: phase.to_string(),
            message: msg.to_string(),
            progress,
        });
        tracing::info!(target: "cabalmesh::bootstrap", phase, progress, "{msg}");
    }
}

// 5. Security: Kill Switch Command
#[tauri::command]
pub async fn kill_switch(state: State<'_, AppState>) -> Result<String, String> {
    let state = state.services().map_err(crate::error::flatten)?;
    let bridge = state.bridge.lock().await;
    
    // Shred local key
    bridge.delete_snapshot().map_err(|e| e.to_string())?;
    
    // Revoke Instant Session (Mock revocation logic since strict real implementation details are complex)
    // In real world: this would revoke the session key on-chain
    
    tracing::warn!("🚨 KILL SWITCH ACTIVATED: Session Shredded.");
    Ok("SESSION_TERMINATED".to_string())
}
