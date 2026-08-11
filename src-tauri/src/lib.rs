mod app_initializer;
mod app_paths;
mod platform;
mod ollama_config;
mod ollama_manager;

// Public because their types *are* the IPC contract: everything below
// serializes across the boundary to the webview, so the shapes are already
// public API whether or not Rust says so. `tests/ipc_contract.rs` pins them.
pub mod agent;
pub mod blockchain_bridge;
pub mod matcher;
pub mod mesh;

/// Request handle onto the mesh actor. See src/mesh_handle.rs.
pub mod mesh_handle;

/// The BLE plane: the offline core, when there is no network at all.
///
/// Separate from [`mesh`] on purpose. That module is the IP plane — gossipsub,
/// mDNS, the relay — and it needs a network to exist. This one needs only the
/// radio in the user's pocket. See docs/ble-mesh-design.md.
pub mod ble;

/// Bootstrap peer configuration. See src/bootstrap_config.rs.
pub mod bootstrap_config;

/// Chain selection and contract addresses. See src/network_config.rs.
pub mod network_config;
pub mod zk_handler;
mod llm_json;
mod lifecycle;
mod telemetry;
mod vault_key;

/// The Android Wi-Fi multicast lock mDNS needs. See src/multicast.rs.
pub mod multicast;

/// Android's platform trust store, which rustls will not start without.
/// See src/tls.rs.
pub mod tls;

/// What this node has actually settled. See src/standing.rs.
pub mod standing;

/// Every intent this device has composed. See src/intents.rs.
pub mod intents;

/// When this installation first ran. See src/install.rs.
pub mod install;

/// Managed application state. See src/state.rs.
pub mod state;

/// Lifecycle for live frontend streams. See src/subscriptions.rs.
pub mod subscriptions;

/// The reshaped command surface. See src/commands.rs.
pub mod commands;

/// Presentation contracts shared with the frontend. See src/bindings.rs.
pub mod bindings;

/// The typed error union that crosses the IPC boundary. See src/error.rs.
pub mod error;

use app_initializer::SystemBootstrap;
use agent::SharkAgent;
use matcher::MatchAgent;
use zk_handler::ZKHandler;
use ollama_manager::OllamaManager;
use blockchain_bridge::BlockchainBridge;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Manager, Emitter};


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // First thing, before anything can emit: on a device this is the only
    // channel that reaches a developer, so nothing useful is logged until it
    // is installed.
    telemetry::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Registers the JNI handle only. Nothing here is reachable over IPC —
        // see src/multicast.rs for why the webview gets no grant for it.
        .plugin(multicast::init())
        // Registered before anything that can make an HTTPS request: on Android
        // rustls has no trust store until this runs. See src/tls.rs.
        .plugin(tls::init())
        // The Android BLE radio. Registers a handle on Android and a no-op
        // elsewhere, so the app builds identically on every platform.
        .plugin(tauri_plugin_cabal_ble::init())
        .setup(|app| {
            // Synchronously, before the webview exists. Bootstrap fills the
            // services in afterwards; until then commands get NotReady rather
            // than a panic from an unmanaged type.
            // Before anything can persist: Tauri knows the correct directory
            // on every platform, and a mobile sandbox has no other right answer.
            match app.path().app_data_dir() {
                Ok(dir) => app_paths::set(dir),
                Err(error) => tracing::error!(
                    target: "cabalmesh::paths",
                    %error,
                    "platform gave no app data directory; falling back"
                ),
            }

            let state = state::AppState::new();
            app.manage(state.clone());

            let app_handle = app.handle().clone();

            // Create consistent Ollama instance
            let ollama_manager = Arc::new(OllamaManager::new(Some("llama2".to_string())));
            let ollama_init = ollama_manager.clone();
            
            // Initialize Ollama in background
            tauri::async_runtime::spawn(async move {
                let ollama = ollama_init;

                // Mobile cannot spawn a local server, so there is nothing to
                // install or start — just check whether the configured remote
                // is reachable.
                if !platform::CAN_SPAWN_PROCESSES {
                    let url = ollama_config::url();
                    tracing::info!("🔍 Checking remote Ollama at {}...", url);
                    if ollama.health_check().await {
                        tracing::info!("✅ Remote Ollama is healthy");
                    } else {
                        tracing::warn!("⚠️  No Ollama at {}", url);
                        tracing::warn!("📝 Set one with the set_ollama_url command or ${}", ollama_config::ENV_VAR);
                    }
                    return;
                }

                tracing::info!("🔍 Checking Ollama installation...");
                if !ollama.is_installed() {
                    tracing::warn!("⚠️  Ollama not found!");
                    tracing::warn!("📝 Please install from: https://ollama.ai");
                    tracing::warn!("   Or run: brew install ollama");
                } else {
                    match ollama.initialize().await {
                        Ok(_) => {
                            tracing::info!("✅ Ollama ready!");
                            for i in 1..=10 {
                                if ollama.health_check().await {
                                    tracing::info!("✅ Ollama service is healthy");
                                    break;
                                }
                                if i == 10 {
                                    tracing::warn!("⚠️  Ollama service not responding");
                                }
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            }
                        }
                        Err(e) => {
                            tracing::error!("❌ Failed to initialize Ollama: {}", e);
                        }
                    }
                }
            });

            // Pass strong reference to mesh setup to store in AppState
            let ollama_state = ollama_manager.clone();

            // Initialize System via Bootstrap Workflow
            tauri::async_runtime::spawn(async move {
                // Shared Bridge Resource (Created here first)
                // Desktop only: there is no .env file in a mobile bundle, and no
                // environment to read it into. Mobile falls through to the
                // compiled-in default until ticket 24 replaces this with a
                // per-network address table.
                #[cfg(desktop)]
                dotenv::dotenv().ok();

                let rpc_url = std::env::var("AVAX_RPC_URL")
                    .unwrap_or_else(|_| blockchain_bridge::DEFAULT_AVAX_RPC_URL.to_string());

                let bridge = Arc::new(Mutex::new(BlockchainBridge::new(Some(rpc_url))));

                // 1. Phase 1
                SystemBootstrap::phase_1_sync(&bridge, &app_handle).await;

                // 2. Phase 2
                SystemBootstrap::phase_2_delegate(&bridge, &app_handle).await;

                // 3. Phase 3 & Network Start
                // The BLE plane starts independently of the swarm, because it
                // is independent: it is what the app does when there is no
                // network for the swarm to use. A failure to start either one
                // must leave the other running.
                let chosen = ble::backend::choose_for_app(&app_handle);
                let ble_transport = chosen
                    .as_ref()
                    .map_or_else(String::new, |t| t.describe().to_string());
                let ble = chosen.map(|transport| {
                    let (handle, mut events) = ble::spawn(
                        transport,
                        ble::fresh_identity(),
                        cabal_ble::peers::Capabilities::none(),
                    );
                    let forward = app_handle.clone();
                    tokio::spawn(async move {
                        while let Some(event) = events.recv().await {
                            let _ = forward.emit("ble-event", format!("{event:?}"));
                        }
                    });
                    handle
                });
                if ble.is_none() {
                    tracing::info!(
                        "no BLE transport for this build; the offline plane is not running \
                         (set {} to link two machines over TCP)",
                        ble::backend::LOOPBACK_ENV
                    );
                }

                match SystemBootstrap::phase_3_network(&app_handle).await {
                    Ok((mut mesh, mesh_handle, mut event_rx, command_rx, event_tx)) => {
                        tracing::info!("✅ System Bootstrap Complete. Mesh Swarm Active.");

                        let relay_bytes = mesh.relay_bytes.clone();

                        // Start Mesh Loop (Background)
                        tokio::spawn(async move {
                            if let Err(e) = mesh.start(event_tx, command_rx).await {
                                tracing::warn!("Mesh network error: {}", e);
                            }
                        });

                        // Forward Mesh Events to Frontend, applying them to the
                        // intent ledger on the way through.
                        //
                        // Applied here rather than polled from a command: the
                        // detail screen has to reflect negotiation as it
                        // happens, and a five-second poll would show a bid
                        // arriving five seconds after the peer sent it.
                        let handle_clone = app_handle.clone();
                        let ledger = state.intents().clone();
                        tokio::spawn(async move {
                            while let Some(event) = event_rx.recv().await {
                                intents::apply_mesh_event(&ledger, &event);
                                let _ = handle_clone.emit("mesh-event", event);
                            }
                        });

                        state.set_services(state::Services {
                            mesh: Some(mesh_handle),
                            ble: ble.clone(),
                            ble_transport: ble_transport.clone(),
                            agent: Arc::new(SharkAgent::new(None)),
                            matcher: Arc::new(MatchAgent::new(None)),
                            zk_handler: Arc::new(ZKHandler::new(None)),
                            ollama: ollama_state,
                            bridge,
                            relay_bytes,
                        });

                        // Only once the mesh is actually participating. The
                        // lock keeps the Wi-Fi radio in a higher-power state,
                        // so taking it before there is anything to discover
                        // would be a battery cost with no benefit.
                        multicast::refresh(&app_handle);
                    }
                    Err(e) => {
                        tracing::error!("❌ Bootstrap Failed: {}", e);
                        // Publish anyway: without a mesh the chain and vault
                        // commands still work, and the UI can say so. Leaving
                        // services unset would make every command NotReady
                        // forever, which reads as a hang rather than an error.
                        state.set_services(state::Services {
                            mesh: None,
                            ble,
                            ble_transport,
                            agent: Arc::new(SharkAgent::new(None)),
                            matcher: Arc::new(MatchAgent::new(None)),
                            zk_handler: Arc::new(ZKHandler::new(None)),
                            ollama: ollama_state,
                            bridge,
                            relay_bytes: Arc::new(AtomicU64::new(0)),
                        });
                    }
                }
            });

            Ok(())
        })
        // One handler, every platform. Desktop used to carry a second,
        // frozen arm for the old RPG UI (see git history for `src/legacy/`);
        // now that UI is gone, desktop and mobile share the same reshaped
        // command surface — new commands join this single list as their
        // screens land, never speculatively.
        .invoke_handler(tauri::generate_handler![
            commands::unsubscribe,
            commands::session_status,
            commands::enter_mesh,
            commands::mesh_snapshot,
            commands::subscribe_mesh_log,
            commands::list_nearby_nodes,
            commands::ble_status,
            commands::list_intents,
            commands::intent_form_options,
            commands::intent_affordability,
            commands::propose_intent,
            commands::preview_intent,
            commands::broadcast_intent,
            commands::intent_detail,
            commands::subscribe_settlement_log,
            commands::settle_intent,
            commands::cancel_intent,
            commands::intent_proof,
            commands::vault_assets,
            commands::vault_modules,
            commands::vault_identities,
            commands::vault_keys,
            commands::profile_summary,
            commands::set_offline_mode,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Mobile lifecycle. Tauri 2.11 propagates these from the platform;
            // 2.9 did not, which is why an earlier plan specified a custom
            // plugin that is no longer needed.
            #[cfg(mobile)]
            if let tauri::RunEvent::WindowEvent { event, .. } = &event {
                match event {
                    tauri::WindowEvent::Suspended => lifecycle::on_suspend(app),
                    tauri::WindowEvent::Resumed => lifecycle::on_resume(app),
                    _ => {}
                }
            }
            let _ = (app, event);
        });
}
