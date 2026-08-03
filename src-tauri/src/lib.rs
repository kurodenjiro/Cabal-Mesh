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

/// Bootstrap peer configuration. See src/bootstrap_config.rs.
pub mod bootstrap_config;

/// Chain selection and contract addresses. See src/network_config.rs.
pub mod network_config;
pub mod zk_handler;
mod llm_json;
mod lifecycle;
mod telemetry;
mod vault_key;

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

/// The frozen desktop IPC surface. See `legacy/mod.rs` for why it exists and
/// what it is not allowed to become.
#[cfg(all(desktop, feature = "desktop-legacy"))]
pub mod legacy;


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

                        // Forward Mesh Events to Frontend
                        let handle_clone = app_handle.clone();
                        tokio::spawn(async move {
                            while let Some(event) = event_rx.recv().await {
                                let _ = handle_clone.emit("mesh-event", event);
                            }
                        });

                        state.set_services(state::Services {
                            mesh: Some(mesh_handle),
                            agent: Arc::new(SharkAgent::new(None)),
                            matcher: Arc::new(MatchAgent::new(None)),
                            zk_handler: Arc::new(ZKHandler::new(None)),
                            ollama: ollama_state,
                            bridge,
                            relay_bytes,
                        });
                    }
                    Err(e) => {
                        tracing::error!("❌ Bootstrap Failed: {}", e);
                        // Publish anyway: without a mesh the chain and vault
                        // commands still work, and the UI can say so. Leaving
                        // services unset would make every command NotReady
                        // forever, which reads as a hang rather than an error.
                        state.set_services(state::Services {
                            mesh: None,
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
        // Handler registration is split by surface.
        //
        // The frozen desktop commands are compiled and registered only on
        // desktop with the `desktop-legacy` feature. The mobile handler never
        // sees them, which is what keeps `capabilities/mobile.json` able to
        // grant nothing.
        //
        // The reshaped screen commands are added to the mobile arm as they are
        // built (tickets 29 onward), never speculatively.
        .invoke_handler({
            #[cfg(all(desktop, feature = "desktop-legacy"))]
            {
                tauri::generate_handler![
                    commands::unsubscribe,
                    commands::session_status,
                    commands::enter_mesh,
                    commands::mesh_snapshot,
                    commands::subscribe_mesh_log,
                    commands::list_nearby_nodes,
                    commands::list_intents,
                    commands::intent_form_options,
                    commands::preview_intent,
                    commands::vault_assets,
                    commands::vault_identities,
                    commands::vault_keys,
                    commands::profile_summary,
                    commands::set_offline_mode,
                    app_initializer::kill_switch,
            legacy::send_intent_to_mesh,
            legacy::analyze_pdf_content,
            legacy::generate_zk_proof,
            legacy::sync_blockchain_state,
            legacy::enable_instant_session,
            legacy::create_escrow,
            legacy::release_escrow,
            legacy::refund_escrow,
            legacy::get_escrow_status,
            legacy::get_bridge_status,
            legacy::check_rpc_reachable,
            legacy::get_wallet_snapshot,
            legacy::delete_wallet_snapshot,
            legacy::get_identity,
            legacy::get_primary_private_key,
            legacy::logout_wallet,
            legacy::import_wallet,
            legacy::mint_voucher,
            legacy::approve_voucher,
            legacy::create_asset_listing,
            legacy::get_active_asset_listings,
            legacy::buy_listing,
            legacy::release_deal,
            legacy::refund_deal,
            legacy::submit_raw_transaction,
            legacy::get_pending_relay_txs,
            legacy::prune_stale_relay_txs,
            legacy::mark_relay_tx_status,
            legacy::record_relayed_tx,
            legacy::get_relayed_history,
            legacy::get_relay_boost,
            legacy::apply_relay_boost,
            legacy::redeem_voucher,
            legacy::get_voucher_owner,
            legacy::get_owned_vouchers,
            legacy::get_my_deals,
            legacy::get_ollama_status,
            legacy::get_ollama_url,
            legacy::set_ollama_url,
            legacy::can_spawn_processes,
            legacy::extract_pdf_text,
            legacy::sign_content,
            legacy::store_content,
            legacy::get_content,
            legacy::receive_content,
            legacy::get_received_content,
            legacy::match_intent_to_listings,
            legacy::get_relay_stats,
            legacy::record_relay_bytes
                ]
            }
            #[cfg(not(all(desktop, feature = "desktop-legacy")))]
            {
                // Mobile gets the reshaped surface only. Screen commands join
                // it as their screens land.
                tauri::generate_handler![commands::unsubscribe, commands::session_status, commands::enter_mesh, commands::mesh_snapshot, commands::subscribe_mesh_log, commands::list_nearby_nodes, commands::list_intents, commands::intent_form_options, commands::preview_intent, commands::vault_assets, commands::vault_identities, commands::vault_keys, commands::profile_summary, commands::set_offline_mode]
            }
        })
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
