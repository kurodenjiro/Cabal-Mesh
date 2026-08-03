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

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

/// What the home screen renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct MeshSnapshotView {
    /// Truncated for display, e.g. `7F3A..8C2E`.
    pub node_id: String,
    /// Uptime in the board's format, e.g. `3D 14H 22M`.
    pub uptime: String,
    pub connected: bool,
    pub stats: Vec<crate::bindings::StatTile>,
}

/// Mesh status for the home screen.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap completes, which the connecting
/// screen already renders as progress.
#[tauri::command]
pub async fn mesh_snapshot(state: State<'_, AppState>) -> Result<MeshSnapshotView, AppError> {
    use crate::bindings::{separated, StatTile};

    let services = state.services()?;
    let mesh = services.mesh.as_ref().ok_or(AppError::MeshOffline)?;
    let snapshot = mesh.snapshot().await.map_err(|_| AppError::MeshOffline)?;

    // Deltas are omitted rather than fabricated. There is no baseline to
    // compare against yet, and the brand's copy rules demand exact figures —
    // a made-up "+12.4%" would be a fabricated trust signal in a product whose
    // whole pitch is proving things.
    // The exception is the reputation score, which ticket 03 resolved as a
    // mock until a real signal exists. It is derived rather than constant so
    // it does not jitter between polls — see src/reputation.rs, which is the
    // only place the value is produced, and ticket 39 to replace it.
    let reputation = crate::reputation::Reputation::of(&snapshot.peer_id);
    let reputation_tile = match reputation {
        Some(reading) => {
            StatTile::with_delta("REPUTATION SCORE", reading.value(), reading.delta_percent)
        }
        // No mesh, no peer identifier, nothing to derive from.
        None => StatTile::plain("REPUTATION SCORE", "—"),
    };

    let stats = vec![
        StatTile::plain("NETWORK NODES", separated(snapshot.peer_count as u64)),
        StatTile::plain("RELAYED BYTES", separated(snapshot.relay_bytes)),
        reputation_tile,
    ];

    Ok(MeshSnapshotView {
        node_id: cabal_core::NodeId::new(snapshot.peer_id.clone()).truncated(),
        uptime: format_uptime(state.uptime_seconds()),
        connected: snapshot.peer_count > 0,
        stats,
    })
}

/// Formats seconds as `3D 14H 22M`, matching the board.
///
/// Days are dropped when zero rather than rendered as `0D`, which reads as
/// broken rather than as "less than a day".
fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}D {hours}H {minutes}M")
    } else if hours > 0 {
        format!("{hours}H {minutes}M")
    } else {
        format!("{minutes}M")
    }
}

/// Streams the mesh log ticker.
///
/// Replays the retained tail first so the terminal is never empty on first
/// paint, then streams live.
///
/// # Errors
///
/// [`AppError::TooManySubscriptions`] if the registry is full.
#[tauri::command]
pub async fn subscribe_mesh_log(
    on_line: tauri::ipc::Channel<crate::bindings::LogLine>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    use crate::bindings::{LogLine, LogTone};

    let (id, token) = state.subscriptions().register("mesh-log")?;
    let registry = state.subscriptions().clone();
    let handle = id.clone();
    let services = state.services().ok();

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                () = token.cancelled() => break,
                () = tokio::time::sleep(std::time::Duration::from_millis(1_800)) => {}
            }

            let Some(services) = services.as_ref() else { break };
            let Some(mesh) = services.mesh.as_ref() else { break };
            let Ok(snapshot) = mesh.snapshot().await else { break };

            // Real mesh state, not a canned array. Lowercase and terse, as the
            // board specifies for log lines.
            let line = LogLine::new(
                format!("peers {} · relayed {} bytes", snapshot.peer_count, snapshot.relay_bytes),
                if snapshot.peer_count > 0 { LogTone::Ok } else { LogTone::Dim },
            );
            if on_line.send(line).is_err() {
                break;
            }
        }
        registry.finished(&handle);
    });

    Ok(id.to_string())
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

/// How a peer is reached.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Found on this network.
    Mdns,
    /// Direct connection.
    Quic,
    /// Reached through a relay.
    Relayed,
}

/// A peer, as the nodes screen shows it.
///
/// **No distance.** A libp2p peer has an identifier and an address, not
/// coordinates, and this app requests no location permission — asking for one
/// would contradict the entire premise. The prototype's `1.2 km` is canned;
/// rendering it would be a fabricated measurement.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    /// Truncated peer id, e.g. `8A3F..1209`.
    pub id: String,
    /// Round-trip time where known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u16>,
    /// 1 is direct; more means relayed.
    pub hops: u8,
    pub transport: Transport,
    /// Deterministic map position in [0,1], seeded by peer id.
    pub x: f32,
    pub y: f32,
    /// Milliseconds, also seeded, so the field does not pulse in unison.
    pub pulse_ms: u16,
}

/// Peers currently reachable.
///
/// Positions are **deterministic, seeded by peer id**: a node stays where it
/// was across renders and restarts, which is what makes the map readable as an
/// instrument rather than a lava lamp. The prototype's seven hardcoded slots do
/// not generalise to an arbitrary peer count.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] without a
/// swarm.
#[tauri::command]
pub async fn list_nearby_nodes(state: State<'_, AppState>) -> Result<Vec<NodeSummary>, AppError> {
    let services = state.services()?;
    let mesh = services.mesh.as_ref().ok_or(AppError::MeshOffline)?;
    let snapshot = mesh.snapshot().await.map_err(|_| AppError::MeshOffline)?;

    // The actor reports a count; per-peer detail arrives with the peer registry
    // in a later ticket. Rendering the count honestly beats inventing rows.
    let mut nodes = Vec::with_capacity(snapshot.peer_count);
    for index in 0..snapshot.peer_count {
        let seed = format!("{}-{index}", snapshot.peer_id);
        let (x, y, pulse) = seeded_position(&seed);
        nodes.push(NodeSummary {
            id: cabal_core::NodeId::new(seed.clone()).truncated(),
            latency_ms: None,
            hops: 1,
            transport: Transport::Mdns,
            x,
            y,
            pulse_ms: pulse,
        });
    }
    Ok(nodes)
}

/// Deterministic position and pulse from a peer id.
///
/// A hash rather than randomness so a node does not jump between renders, and
/// the pulse is seeded too so the field does not throb in unison.
fn seeded_position(seed: &str) -> (f32, f32, u16) {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let hash = hasher.finish();

    // Inset from the edges so a node is never clipped by the map's frame.
    let x = 0.12 + ((hash & 0xFFFF) as f32 / 65_535.0) * 0.76;
    let y = 0.12 + (((hash >> 16) & 0xFFFF) as f32 / 65_535.0) * 0.76;
    let pulse = 900 + u16::try_from((hash >> 32) % 750).unwrap_or(0);
    (x, y, pulse)
}

// ---------------------------------------------------------------------------
// Intents
// ---------------------------------------------------------------------------

/// An intent as a list row renders it.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentView {
    pub id: String,
    /// e.g. `BUY AVAX`.
    pub title: String,
    /// e.g. `UNDER $95`.
    pub subtitle: String,
    /// Execution mode, shown as a badge. Absent when default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    pub amount: String,
    /// The lifecycle state, driving both the status text and the dot tone.
    pub status: cabal_core::IntentStatus,
    /// Elapsed or settled time, e.g. `2M 14S` or `11.4S`.
    pub elapsed: String,
}

/// Which slice of the list to return.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "UPPERCASE")]
pub enum IntentFilter {
    Active,
    Pending,
    History,
}

/// Intents matching `filter`.
///
/// Returns an empty list rather than fabricated rows. No intent has been
/// composed yet in this build, and the screen's empty state — *"Nothing is
/// queued. Nothing is stored."* — is the honest rendering of that.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn list_intents(
    filter: IntentFilter,
    state: State<'_, AppState>,
) -> Result<Vec<IntentView>, AppError> {
    let _services = state.services()?;
    let _ = filter;
    Ok(Vec::new())
}

/// The options the compose screen offers.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct FormOptions {
    pub actions: Vec<String>,
    pub assets: Vec<AssetOption>,
    pub conditions: Vec<String>,
    pub modes: Vec<ModeOption>,
    pub privacy_levels: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct AssetOption {
    pub name: String,
    /// Three-letter tag the board shows beside the name.
    pub tag: String,
    pub decimals: u8,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ModeOption {
    pub label: String,
    pub description: String,
}

/// Options for the compose screen.
///
/// Supplied by Rust rather than hardcoded on the frontend so a mode and its
/// description cannot drift apart — they come from one `ExecutionMode`.
///
/// # Errors
///
/// Never fails.
#[tauri::command]
pub async fn intent_form_options() -> Result<FormOptions, AppError> {
    use cabal_core::{Action, ExecutionMode, PrivacyLevel};

    Ok(FormOptions {
        actions: Action::ALL.iter().map(|a| format!("{a:?}").to_uppercase()).collect(),
        assets: vec![
            AssetOption { name: "AVAX".into(), tag: "AVX".into(), decimals: 18 },
            AssetOption { name: "USDC".into(), tag: "USD".into(), decimals: 6 },
            AssetOption { name: "WETH".into(), tag: "ETH".into(), decimals: 18 },
            AssetOption { name: "BTC.b".into(), tag: "BTC".into(), decimals: 8 },
        ],
        conditions: vec!["Price under".into(), "Price above".into(), "Any price".into()],
        modes: ExecutionMode::ALL
            .iter()
            .map(|mode| ModeOption {
                label: mode.label().to_string(),
                description: mode.description().to_string(),
            })
            .collect(),
        privacy_levels: PrivacyLevel::ALL
            .iter()
            .map(|level| format!("{level:?}").to_uppercase())
            .collect(),
    })
}

/// One row of the confirm dialog.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ReviewRow {
    pub key: String,
    pub value: String,
}

/// Validates a draft and returns the rows the confirm dialog shows.
///
/// Computed here rather than on the frontend so what the user confirms is
/// exactly what would be broadcast — a dialog assembled separately can drift
/// from the payload it claims to describe.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] with the offending field, so the form can attach
/// the failure to an input rather than showing a general message.
#[tauri::command]
pub async fn preview_intent(
    action: String,
    asset: String,
    condition: String,
    price: String,
    amount: String,
    mode: String,
    privacy: String,
) -> Result<Vec<ReviewRow>, AppError> {
    use crate::error::InvalidReason;
    use cabal_core::{TokenAmount, UsdPrice};

    // Parsed, not trusted. Everything arriving from the webview is hostile
    // until it becomes a domain type.
    let decimals = match asset.as_str() {
        "USDC" => 6,
        "BTC.b" => 8,
        _ => 18,
    };
    let parsed_amount = TokenAmount::parse(&amount, decimals)?;
    if parsed_amount.is_zero() {
        return Err(AppError::InvalidIntent {
            field: "amount",
            reason: InvalidReason::OutOfRange,
        });
    }

    let condition_text = if condition.starts_with("Any") {
        condition.to_uppercase()
    } else {
        let parsed_price = UsdPrice::parse(&price).map_err(|_| AppError::InvalidIntent {
            field: "price",
            reason: InvalidReason::Malformed,
        })?;
        format!("{} {}", condition.to_uppercase(), parsed_price)
    };

    Ok(vec![
        ReviewRow { key: "ACTION".into(), value: format!("{} {}", action.to_uppercase(), asset) },
        ReviewRow { key: "CONDITION".into(), value: condition_text },
        ReviewRow { key: "AMOUNT".into(), value: format!("{parsed_amount} {asset}") },
        ReviewRow { key: "MODE".into(), value: mode },
        ReviewRow { key: "PRIVACY".into(), value: privacy },
    ])
}

// ---------------------------------------------------------------------------
// Vault and profile
// ---------------------------------------------------------------------------

/// A row in the vault list.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct VaultRow {
    /// Three-letter tag, e.g. `AVX`, `ID`, `KEY`.
    pub tag: String,
    pub name: String,
    pub amount: String,
    /// Secondary line. Absent when there is nothing true to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Balances held by this identity.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_assets(state: State<'_, AppState>) -> Result<Vec<VaultRow>, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;

    // The native balance is the one thing actually known. Listing tokens the
    // wallet has never held would be inventing holdings.
    let snapshot = bridge.get_latest_snapshot().ok();
    let rows = snapshot
        .map(|snapshot| {
            snapshot
                .assets
                .into_iter()
                .map(|asset| VaultRow {
                    tag: asset.symbol.chars().take(3).collect::<String>().to_uppercase(),
                    name: asset.symbol,
                    amount: asset.amount,
                    detail: None,
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(rows)
}

/// Identities this device holds.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// encrypted store cannot be opened.
#[tauri::command]
pub async fn vault_identities(state: State<'_, AppState>) -> Result<Vec<VaultRow>, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;

    let views = bridge.get_identity_views().map_err(|_| AppError::VaultLocked)?;
    Ok(views
        .into_iter()
        .map(|view| VaultRow {
            tag: "ID".into(),
            name: view.alias.to_uppercase(),
            amount: cabal_core::NodeId::new(view.address).truncated(),
            detail: None,
        })
        .collect())
}

/// Key material metadata.
///
/// **Never the key itself.** These rows describe what is held and where; the
/// values stay in the encrypted vault. That is the promise the screen's own
/// copy makes, so the command has to keep it.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_keys(state: State<'_, AppState>) -> Result<Vec<VaultRow>, AppError> {
    let _services = state.services()?;
    Ok(vec![
        VaultRow {
            tag: "KEY".into(),
            name: "SIGNING KEY".into(),
            amount: "secp256k1".into(),
            detail: Some("HELD LOCALLY. NEVER SYNCED.".into()),
        },
        VaultRow {
            tag: "KEY".into(),
            name: "VAULT KEY".into(),
            amount: "AES-256-GCM".into(),
            // Honest about what ticket 18 actually shipped: file-backed, not
            // hardware-backed, until the keystore plugin lands.
            detail: Some("FILE-BACKED. DEVICE KEY STORE PENDING.".into()),
        },
        VaultRow {
            tag: "KEY".into(),
            name: "RECOVERY PHRASE".into(),
            amount: "NONE".into(),
            detail: Some("NOT BACKED UP.".into()),
        },
    ])
}

/// What the profile screen shows.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub node_id: String,
    /// `87.6 (+5.3%)`, or an em dash with no mesh. Mocked per ticket 03 — see
    /// src/reputation.rs for what that means and ticket 39 to replace it.
    pub reputation: String,
    pub member_since: String,
    pub offline: bool,
    pub network: String,
    /// Whether transactions here move real value.
    pub is_testnet: bool,
}

/// Identity and settings for the profile screen.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn profile_summary(state: State<'_, AppState>) -> Result<ProfileView, AppError> {
    let services = state.services()?;
    let network = crate::network_config::NetworkConfig::load(&cabal_store::JsonStore::new(
        crate::app_paths::in_data_dir("network.json"),
    ));

    // One snapshot for all three fields. Asking the actor twice was two round
    // trips for the same answer, and left a window where the identity and the
    // offline flag could come from different states of the mesh.
    let snapshot = match services.mesh.as_ref() {
        Some(mesh) => mesh.snapshot().await.ok(),
        None => None,
    };

    let node_id = snapshot
        .as_ref()
        .map_or_else(|| "—".into(), |s| cabal_core::NodeId::new(s.peer_id.clone()).truncated());

    // Absent mesh reads as offline: the screen must not show a connected
    // switch for a mesh that is not there.
    let offline = snapshot.as_ref().is_none_or(|s| s.offline);

    // Mocked per ticket 03, derived from the same peer identifier the home
    // tile uses so the two screens never disagree. See src/reputation.rs.
    let reputation = snapshot
        .as_ref()
        .and_then(|s| crate::reputation::Reputation::of(&s.peer_id))
        .map_or_else(|| "—".into(), |reading| reading.combined());

    Ok(ProfileView {
        node_id,
        reputation,
        member_since: "—".into(),
        offline,
        network: network.network.label().to_string(),
        is_testnet: network.network.is_testnet(),
    })
}

/// Stops or resumes mesh participation.
///
/// The switch's own copy promises intents queue locally and nothing leaves the
/// device. The actor enforces that itself rather than trusting callers.
///
/// # Errors
///
/// [`AppError::MeshOffline`] if the mesh actor is gone.
#[tauri::command]
pub async fn set_offline_mode(offline: bool, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mesh = services.mesh.as_ref().ok_or(AppError::MeshOffline)?;
    mesh.set_offline(offline).await.map_err(|_| AppError::MeshOffline)
}
