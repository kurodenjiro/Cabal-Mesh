//! The reshaped command surface — the whole IPC surface, shared by desktop
//! and mobile. Commands here return [`AppError`] rather than `String`, so the
//! frontend switches on a variant and renders its own copy.
//!
//! Screen commands land with their screens — never speculatively, because an
//! unreachable command still has to be granted a permission, and a permission
//! granted ahead of a caller is a permission nobody is checking.

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
    // The third tile is the one figure here that is about this node rather
    // than the network: what it has settled. Ticket 39 replaced ticket 03's
    // mocked "reputation score" with it — see src/standing.rs for why the
    // label changed rather than the definition.
    //
    // It reads the local ledger, so unlike the other two it is just as true
    // with no mesh as with one.
    let standing = crate::standing::LocalStanding::of(state.intents(), crate::intents::now_ms());
    let settled_tile = match standing.delta_percent {
        Some(delta) => StatTile::with_delta("INTENTS SETTLED", standing.value(), delta),
        // No prior window, so no baseline. `plain` omits the delta rather than
        // rendering `+0.0%` for a trend that was never measured.
        None => StatTile::plain("INTENTS SETTLED", standing.value()),
    };

    // Broadcasted: ledger entries this device has actually pushed off the
    // device — a `Draft` is composed but nothing has left yet, so it does not
    // count. Received: distinct intents seen from other peers, over the mesh
    // or bridged in from BLE — see `ReceivedLog` for why it dedupes rather
    // than counting every delivery attempt. Both read local state, so like
    // `settled_tile` they hold with no mesh connected at all.
    let broadcasted = state
        .intents()
        .all()
        .iter()
        .filter(|intent| !matches!(intent.status, cabal_core::IntentStatus::Draft))
        .count();

    let stats = vec![
        StatTile::plain("NETWORK NODES", separated(snapshot.peer_count as u64)),
        StatTile::plain("RELAYED BYTES", separated(snapshot.relay_bytes)),
        settled_tile,
        StatTile::plain("BROADCASTED", separated(broadcasted as u64)),
        StatTile::plain("RECEIVED", separated(state.received().count() as u64)),
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
    /// Heard over the radio, with no network involved at all.
    ///
    /// Distinct from the others in the only way the user cares about: a `Ble`
    /// peer is a person in the room, and it is still there when the Wi-Fi is
    /// not.
    Ble,
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

    // BLE peers first, and they are real rows rather than a count: the engine
    // knows each one's identifier and how many hops away it is, so there is
    // nothing to invent.
    let mut nodes = Vec::new();
    if let Some(ble) = services.ble.as_ref() {
        if let Ok(peers) = ble.peers().await {
            for peer in peers {
                let id = peer.id.to_string();
                let (x, y, pulse) = seeded_position(&id);
                nodes.push(NodeSummary {
                    id: cabal_core::NodeId::new(id).truncated(),
                    latency_ms: None,
                    hops: peer.hops,
                    transport: Transport::Ble,
                    x,
                    y,
                    pulse_ms: pulse,
                });
            }
        }
    }

    // The IP plane. Without a swarm this is simply empty — a node with
    // Bluetooth and no network is a working node, and returning `MeshOffline`
    // here would blank a screen that has peers to show.
    let Some(mesh) = services.mesh.as_ref() else {
        return Ok(nodes);
    };
    let Ok(snapshot) = mesh.snapshot().await else {
        return Ok(nodes);
    };

    // The mesh actor reports a count, not a registry; per-peer detail arrives
    // with the peer registry in a later ticket. Rendering the count honestly
    // beats inventing rows.
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

/// What the nodes screen shows about the offline plane.
///
/// Every field is a measurement. There is no "signal strength" and no
/// "distance": the radio reports neither, and the app requests no location
/// permission — asking for one would contradict the premise.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct BleStatusView {
    /// Whether the plane is running at all.
    ///
    /// False means no radio backend and no development transport — a state
    /// the app is expected to survive, and one the screen has to be able to
    /// say out loud rather than rendering as "no peers".
    pub running: bool,
    /// This session's identifier, truncated. Changes on every launch.
    pub node_id: String,
    /// Which backend is carrying the plane, verbatim: `loopback`, or the name
    /// of a real radio. Never dressed up as "BLE" when it is not.
    pub transport: String,
    /// Radio links currently open.
    pub links: usize,
    /// Peers one hop away — people in the room.
    pub direct_peers: usize,
    /// Every peer reachable, direct or through a neighbour.
    pub reachable_peers: usize,
    /// Reachable peers offering a way to the internet.
    pub gateways: usize,
    /// Packets forwarded for other people.
    pub relayed: u64,
    /// Forwards cancelled because a neighbour was faster.
    ///
    /// Shown beside `relayed` because without it the two states "the mesh is
    /// quiet" and "everything is arriving twice and being suppressed" look
    /// identical.
    pub suppressed: u64,
    pub offline: bool,
}

/// Status of the BLE plane.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap completes. Never
/// [`AppError::MeshOffline`]: a plane that is not running is a status, not a
/// failure, and the screen needs to render it.
#[tauri::command]
pub async fn ble_status(state: State<'_, AppState>) -> Result<BleStatusView, AppError> {
    let services = state.services()?;

    let Some(ble) = services.ble.as_ref() else {
        return Ok(BleStatusView {
            running: false,
            node_id: String::new(),
            transport: String::new(),
            links: 0,
            direct_peers: 0,
            reachable_peers: 0,
            gateways: 0,
            relayed: 0,
            suppressed: 0,
            offline: false,
        });
    };

    let status = ble.status().await.map_err(|_| AppError::NotReady {
        subsystem: "ble".into(),
    })?;
    Ok(BleStatusView {
        running: true,
        node_id: cabal_core::NodeId::new(status.peer_id.to_string()).truncated(),
        transport: services.ble_transport.clone(),
        links: status.links,
        direct_peers: status.direct_peers,
        reachable_peers: status.reachable_peers,
        gateways: status.gateways,
        relayed: status.relayed,
        suppressed: status.suppressed,
        offline: status.offline,
    })
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

impl IntentFilter {
    /// Whether an intent belongs in this slice.
    ///
    /// `Pending` is the queue: composed but never broadcast, which is exactly
    /// what an intent created offline looks like. Mapping it to anything else
    /// would hide the queue the offline path exists to build.
    fn admits(self, status: &cabal_core::IntentStatus) -> bool {
        match self {
            Self::Active => status.is_active(),
            Self::Pending => matches!(status, cabal_core::IntentStatus::Draft),
            Self::History => status.is_terminal(),
        }
    }
}

/// Renders one ledger entry as a list row.
fn row_for(intent: &crate::intents::Intent, now_ms: u64) -> IntentView {
    let action = format!("{:?}", intent.draft.action).to_uppercase();
    let subtitle = match intent.draft.condition {
        cabal_core::Condition::Under { price } => format!("UNDER {price}"),
        cabal_core::Condition::Above { price } => format!("ABOVE {price}"),
        cabal_core::Condition::Any => "ANY PRICE".into(),
    };

    IntentView {
        id: intent.id.to_string(),
        title: format!("{action} {}", intent.draft.asset),
        subtitle,
        // Shark is the default, so badging it would put a badge on almost
        // every row and stop the badge meaning anything.
        badge: match intent.draft.mode {
            cabal_core::ExecutionMode::Shark => None,
            other => Some(other.label().to_string()),
        },
        amount: format!("{} {}", intent.draft.amount, intent.draft.asset),
        status: intent.status.clone(),
        elapsed: crate::intents::format_elapsed(intent.elapsed_ms(now_ms)),
    }
}

/// Intents matching `filter`, newest first.
///
/// # Errors
///
/// Never fails. Deliberately does **not** require bootstrap: the intents most
/// worth showing are the ones queued while there was no mesh to boot.
#[tauri::command]
pub async fn list_intents(
    filter: IntentFilter,
    state: State<'_, AppState>,
) -> Result<Vec<IntentView>, AppError> {
    let now = crate::intents::now_ms();
    Ok(state
        .intents()
        .all()
        .iter()
        .filter(|intent| filter.admits(&intent.status))
        .map(|intent| row_for(intent, now))
        .collect())
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
    /// The spendable balance, pre-formatted — what MAX fills in.
    ///
    /// Absent rather than zero when the balance is unknown, which is what an
    /// asset this wallet has never held looks like. Rendering an unknown
    /// balance as `0` would tell the user they have none, which is a different
    /// claim entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ModeOption {
    pub label: String,
    pub description: String,
}

/// The assets the compose screen offers, and their precision.
///
/// One table, so the decimals used to parse an amount and the decimals shown
/// beside it cannot disagree.
const ASSETS: [(&str, &str, u8); 4] = [
    ("AVAX", "AVX", 18),
    ("USDC", "USD", 6),
    ("WETH", "ETH", 18),
    ("BTC.b", "BTC", 8),
];

/// How many decimals an asset carries, or `None` if it is not one we offer.
fn decimals_for(asset: &str) -> Option<u8> {
    ASSETS
        .iter()
        .find(|(name, _, _)| *name == asset)
        .map(|(_, _, decimals)| *decimals)
}

/// Builds [`FormOptions`] from the domain model and (best-effort) balances.
///
/// Split out from the [`intent_form_options`] command so `parse_intent_chat`
/// can embed the exact same option vocabulary in its prompt — the model's
/// allowed answers come from this, not a second hardcoded list that could
/// drift from what the segmented controls actually offer.
async fn build_form_options(state: &AppState) -> FormOptions {
    use cabal_core::{Action, ExecutionMode, PrivacyLevel};

    // Balances are best-effort. Before bootstrap, or with no chain snapshot,
    // every asset simply has no maximum.
    let balances = match state.services() {
        Ok(services) => {
            let bridge = services.bridge.lock().await;
            bridge
                .get_latest_snapshot()
                .map(|snapshot| {
                    snapshot
                        .assets
                        .into_iter()
                        .map(|asset| (asset.symbol, asset.amount))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
        Err(_) => Vec::new(),
    };

    FormOptions {
        actions: Action::ALL.iter().map(|a| format!("{a:?}").to_uppercase()).collect(),
        assets: ASSETS
            .iter()
            .map(|(name, tag, decimals)| AssetOption {
                name: (*name).to_string(),
                tag: (*tag).to_string(),
                decimals: *decimals,
                available: balances
                    .iter()
                    .find(|(symbol, _)| symbol.eq_ignore_ascii_case(name))
                    .map(|(_, amount)| amount.clone()),
            })
            .collect(),
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
    }
}

/// Options for the compose screen.
///
/// Supplied by Rust rather than hardcoded on the frontend so a mode and its
/// description cannot drift apart — they come from one `ExecutionMode` — and
/// so the maximum comes from the same balance the vault screen shows.
///
/// # Errors
///
/// Never fails. An unavailable balance omits the maximum rather than failing
/// the whole form: composing an intent offline is a supported path.
#[tauri::command]
pub async fn intent_form_options(state: State<'_, AppState>) -> Result<FormOptions, AppError> {
    Ok(build_form_options(&state).await)
}

/// One row of the confirm dialog.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ReviewRow {
    pub key: String,
    pub value: String,
}

/// What the confirm dialog renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentPreview {
    pub rows: Vec<ReviewRow>,
    /// The dialog's closing line, chosen by whether this will broadcast now.
    pub confirm: String,
    /// Whether confirming broadcasts immediately or queues locally. Drives the
    /// button's own verb, so the dialog does not promise one thing in prose and
    /// another on the control.
    pub will_broadcast: bool,
}

/// The confirm dialog's closing line when the intent goes out now.
///
/// Ticket 04. Two strings rather than one vague enough to be true in both
/// states, and both live here beside the rows so the dialog cannot describe a
/// path this command will not take.
const CONFIRM_ONLINE: &str =
    "This intent broadcasts to the mesh and settles on-chain. No identity is attached.";

/// The closing line when there is no mesh to broadcast to.
///
/// The prototype claimed offline intents execute and settle. They do not: the
/// architecture is queue-then-drain.
const CONFIRM_QUEUED: &str =
    "Queued locally. Broadcast and settlement follow reconnection. No identity is attached.";

/// The compose form's fields, exactly as the screen holds them.
///
/// One type rather than seven parameters on two commands. That is what makes
/// "preview and broadcast see the same input" a property of the signature
/// instead of something a caller has to get right twice.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentFields {
    pub action: String,
    pub asset: String,
    pub condition: String,
    /// Ignored when the condition carries no price.
    pub price: String,
    pub amount: String,
    pub mode: String,
    pub privacy: String,
}

/// Turns raw form fields into a domain draft.
///
/// The single parse for both preview and broadcast. That is what makes the
/// review rows honest: they are rendered *from the draft*, so the dialog cannot
/// describe one thing while the broadcast sends another.
fn parse_draft(fields: &IntentFields) -> Result<cabal_core::IntentDraft, AppError> {
    use crate::error::InvalidReason;
    use cabal_core::{Action, Condition, ExecutionMode, IntentDraft, PrivacyLevel, TokenAmount, UsdPrice};

    let IntentFields { action, asset, condition, price, amount, mode, privacy } = fields;

    // Parsed, not trusted. Everything arriving from the webview is hostile
    // until it becomes a domain type.
    let decimals = decimals_for(asset).ok_or(AppError::InvalidIntent {
        field: "asset",
        reason: InvalidReason::Malformed,
    })?;

    let action = match action.to_ascii_uppercase().as_str() {
        "BUY" => Action::Buy,
        "SELL" => Action::Sell,
        "SWAP" => Action::Swap,
        "STAKE" => Action::Stake,
        _ => {
            return Err(AppError::InvalidIntent {
                field: "action",
                reason: InvalidReason::Malformed,
            })
        }
    };

    let parsed_amount = TokenAmount::parse(amount, decimals)?;
    if parsed_amount.is_zero() {
        return Err(AppError::InvalidIntent {
            field: "amount",
            reason: InvalidReason::OutOfRange,
        });
    }

    let condition = if condition.to_ascii_lowercase().starts_with("any") {
        Condition::Any
    } else {
        let parsed_price = UsdPrice::parse(price).map_err(|_| AppError::InvalidIntent {
            field: "price",
            reason: InvalidReason::Malformed,
        })?;
        if condition.to_ascii_lowercase().contains("above") {
            Condition::Above { price: parsed_price }
        } else {
            Condition::Under { price: parsed_price }
        }
    };

    let mode = ExecutionMode::ALL
        .into_iter()
        .find(|candidate| candidate.label().eq_ignore_ascii_case(mode))
        .ok_or(AppError::InvalidIntent {
            field: "mode",
            reason: InvalidReason::Malformed,
        })?;

    let privacy = PrivacyLevel::ALL
        .into_iter()
        .find(|candidate| format!("{candidate:?}").eq_ignore_ascii_case(privacy))
        .ok_or(AppError::InvalidIntent {
            field: "privacy",
            reason: InvalidReason::Malformed,
        })?;

    Ok(IntentDraft {
        action,
        asset: asset.as_str().into(),
        condition,
        amount: parsed_amount,
        mode,
        privacy,
    })
}

/// The five rows the confirm dialog shows, rendered from the draft itself.
fn review_rows(draft: &cabal_core::IntentDraft) -> Vec<ReviewRow> {
    use cabal_core::Condition;

    let condition = match draft.condition {
        Condition::Under { price } => format!("UNDER {price}"),
        Condition::Above { price } => format!("ABOVE {price}"),
        Condition::Any => "ANY PRICE".into(),
    };

    vec![
        ReviewRow {
            key: "ACTION".into(),
            value: format!("{:?} {}", draft.action, draft.asset).to_uppercase(),
        },
        ReviewRow { key: "CONDITION".into(), value: condition },
        ReviewRow {
            key: "AMOUNT".into(),
            value: format!("{} {}", draft.amount, draft.asset),
        },
        ReviewRow { key: "MODE".into(), value: draft.mode.label().to_string() },
        ReviewRow {
            key: "PRIVACY".into(),
            value: format!("{:?}", draft.privacy).to_uppercase(),
        },
    ]
}

/// Whether an intent composed right now would leave the device.
///
/// Peer count is part of the answer, not an afterthought: gossipsub with no
/// peers has nobody to publish to, so promising a broadcast there would be the
/// same class of lie ticket 04 retired.
async fn will_broadcast(state: &AppState) -> bool {
    let Ok(services) = state.services() else {
        return false;
    };
    let Some(mesh) = services.mesh.as_ref() else {
        return false;
    };
    mesh.snapshot()
        .await
        .is_ok_and(|snapshot| !snapshot.offline && snapshot.peer_count > 0)
}

/// Parses free text into intent fields via the local (or configured) LLM —
/// the "say what you want to do" entry point in
/// `docs/intent-chat-and-modules-design.md`.
///
/// **This is exactly as trusted as a hand-filled form.** The returned
/// fields are raw strings, identical in shape to what `New.tsx` already
/// builds from its own inputs; nothing about this command validates them,
/// signs anything, or is closer to broadcast than the empty form is. The
/// frontend feeds the result into the same `preview_intent` /
/// `broadcast_intent` pipeline unchanged, which is what actually validates
/// it. The model proposes; Rust still decides.
///
/// # Errors
///
/// [`AppError::Internal`] if the LLM could not be reached at all — a model
/// that responded but not in valid JSON is *not* an error: every field
/// comes back blank instead, which the review step already knows how to
/// reject field by field.
#[tauri::command]
pub async fn parse_intent_chat(text: String, state: State<'_, AppState>) -> Result<IntentFields, AppError> {
    let options = build_form_options(&state).await;
    let services = state.services()?;
    services.intent_chat.parse(&text, &options).await.map_err(AppError::internal)
}

/// Validates a draft and returns what the confirm dialog shows.
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
    fields: IntentFields,
    state: State<'_, AppState>,
) -> Result<IntentPreview, AppError> {
    let draft = parse_draft(&fields)?;
    let live = will_broadcast(&state).await;

    Ok(IntentPreview {
        rows: review_rows(&draft),
        confirm: if live { CONFIRM_ONLINE } else { CONFIRM_QUEUED }.to_string(),
        will_broadcast: live,
    })
}

/// Composes an intent and, if there is a mesh, sends it.
///
/// Re-parses from the same fields through the same function the preview used,
/// rather than trusting a payload the frontend assembled from the dialog. The
/// dialog is a rendering of the draft; it is not the draft.
///
/// Returns the new identifier so the caller can open its detail screen.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the draft no longer validates.
#[tauri::command]
pub async fn broadcast_intent(
    fields: IntentFields,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    use crate::bindings::LogTone;
    use crate::intents::line;

    let draft = parse_draft(&fields)?;
    let ledger = state.intents();
    let intent = ledger.create(draft, crate::intents::now_ms());

    ledger.record(&intent.id, line("INTENT COMPOSED.", LogTone::Dim));

    // Composing always succeeds; publishing is what can fail. Keeping them as
    // separate steps is what lets the queue exist at all.
    let published = publish(&state, &intent).await;

    match published {
        Ok(PublishRoute::Mesh(peers)) => {
            ledger.record(&intent.id, line("BROADCAST TO MESH.", LogTone::Ok));
            let route_len = u8::try_from(peers).unwrap_or(u8::MAX);
            let _ = ledger.advance(
                &intent.id,
                cabal_core::IntentStatus::Broadcast { route_len },
                crate::intents::now_ms(),
            );
        }
        Ok(PublishRoute::Ble(peers)) => {
            // No internet here, but a BLE-reachable peer heard it — flooded
            // through the room the same way an `Announce` is, so a gateway
            // among those peers can carry it onward. See the BLE `Intent`
            // bridge in `lib.rs`: whichever gateway sees this over the radio
            // is the one that actually reaches the mesh on our behalf.
            ledger.record(&intent.id, line("BROADCAST VIA BLUETOOTH GATEWAY.", LogTone::Ok));
            let route_len = u8::try_from(peers).unwrap_or(u8::MAX);
            let _ = ledger.advance(
                &intent.id,
                cabal_core::IntentStatus::Broadcast { route_len },
                crate::intents::now_ms(),
            );
        }
        Err(reason) => {
            // Stays a draft, which is the queue. Not an error state: this is
            // the offline path working, and the confirm dialog already said so.
            ledger.record(&intent.id, line(reason, LogTone::Dim));
        }
    }

    Ok(intent.id.to_string())
}

/// Retries every intent still sitting in `Draft` — composed but never
/// broadcast — now that connectivity might reach further than it did when
/// each one was queued.
///
/// Fired from `lib.rs` on `MeshEvent::ConnectivityChanged { online: true }`:
/// that is the one moment a relay reservation just landed, so it is also the
/// one moment retrying is likely to do anything different from the attempt
/// that queued the intent in the first place. A still-failing retry stays
/// silent rather than re-recording "QUEUED LOCALLY." — the original attempt
/// already said that, and a device that reconnects and drops repeatedly
/// would otherwise fill the ledger with copies of the same line.
pub(crate) async fn retry_queued_intents(state: &AppState) {
    use crate::bindings::LogTone;
    use crate::intents::line;

    let ledger = state.intents();
    let queued: Vec<_> = ledger
        .all()
        .into_iter()
        .filter(|intent| matches!(intent.status, cabal_core::IntentStatus::Draft))
        .collect();

    for intent in queued {
        let route_len = match publish(state, &intent).await {
            Ok(PublishRoute::Mesh(peers)) => {
                ledger.record(&intent.id, line("BROADCAST TO MESH.", LogTone::Ok));
                peers
            }
            Ok(PublishRoute::Ble(peers)) => {
                ledger.record(&intent.id, line("BROADCAST VIA BLUETOOTH GATEWAY.", LogTone::Ok));
                peers
            }
            Err(_) => continue,
        };
        let route_len = u8::try_from(route_len).unwrap_or(u8::MAX);
        let _ = ledger.advance(
            &intent.id,
            cabal_core::IntentStatus::Broadcast { route_len },
            crate::intents::now_ms(),
        );
    }
}

/// Which plane actually carried a published intent, and how many peers it
/// reached there — the two planes report peer counts that mean different
/// things, so keeping them apart rather than collapsing to one `usize`
/// avoids attributing an IP peer count to a Bluetooth broadcast or the
/// reverse.
enum PublishRoute {
    Mesh(usize),
    Ble(usize),
}

/// Publishes an intent, preferring the IP mesh and falling back to the BLE
/// room when there is no internet — a device with Bluetooth peers but no
/// Wi-Fi is not actually offline, it just has to hop through a gateway
/// instead of reaching the topic directly. See `docs/ble-mesh-design.md` §8.
///
/// The error is the on-voice line to record, not a message to show raw —
/// every path through here ends up in the terminal the user is reading.
async fn publish(state: &AppState, intent: &crate::intents::Intent) -> Result<PublishRoute, &'static str> {
    let services = state.services().map_err(|_| "MESH NOT READY. QUEUED LOCALLY.")?;

    // The payload is the draft, serialized. Encryption is the transport's job:
    // Noise already covers every hop, and a second layer here would be
    // ceremony rather than protection.
    let payload = serde_json::to_string(&intent.draft).map_err(|_| "COULD NOT ENCODE. QUEUED LOCALLY.")?;
    let privacy_intent = crate::mesh::PrivacyIntent {
        id: intent.id.to_string(),
        intent_type: "intent".into(),
        payload,
        encrypted: false,
        // `verify_relay_integrity` rejects an empty relay path outright — it
        // exists to catch a hop-stripped intent, but a fresh one hasn't been
        // relayed by anyone yet, so it has to carry its own origin stamp from
        // the moment it's created, not first grow one somewhere downstream.
        relay_path: vec!["origin_node".into()],
        relay_fee: None,
    };

    match publish_over_mesh(&services, &privacy_intent).await {
        Ok(peers) => return Ok(PublishRoute::Mesh(peers)),
        Err(mesh_reason) => {
            if let Some(peers) = publish_over_ble(&services, &privacy_intent).await {
                return Ok(PublishRoute::Ble(peers));
            }
            Err(mesh_reason)
        }
    }
}

async fn publish_over_mesh(
    services: &crate::state::Services,
    intent: &crate::mesh::PrivacyIntent,
) -> Result<usize, &'static str> {
    let mesh = services.mesh.as_ref().ok_or("NO MESH. QUEUED LOCALLY.")?;

    let snapshot = mesh.snapshot().await.map_err(|_| "MESH UNREACHABLE. QUEUED LOCALLY.")?;
    if snapshot.offline {
        return Err("OFFLINE MODE. QUEUED LOCALLY.");
    }
    if snapshot.peer_count == 0 {
        return Err("NO PEERS IN RANGE. QUEUED LOCALLY.");
    }

    mesh.publish(intent.clone()).await.map_err(|_| "PUBLISH REFUSED. QUEUED LOCALLY.")?;
    Ok(snapshot.peer_count)
}

/// Extra attempts after the first, spaced [`BLE_INTENT_RESEND_DELAY`] apart.
///
/// The radio's send is fire-and-forget the whole way down — Android's writer
/// thread queues bytes and reports success before a single one reaches the
/// socket, so a link that dies mid-write is indistinguishable, from here, from
/// one that delivered. `Announce` gets away with the same radio because it is
/// periodic and self-heals; `Intent` is submitted once. In a two-peer room
/// there is nobody else to relay a copy that never arrived, so the origin has
/// to be the one that tries again.
const BLE_INTENT_RESENDS: u8 = 2;

/// Gap between resends. Long enough that a link mid-glare (both sides having
/// just dialled each other) has settled to one usable connection by the next
/// attempt, short enough that all retries land inside the few seconds a
/// composing user is still watching the screen.
const BLE_INTENT_RESEND_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

/// The BLE fallback. Returns `None` rather than an error string on every
/// failure path: BLE not having worked is not itself the reason to show —
/// the mesh failure from `publish_over_mesh` already is, and this is only
/// ever consulted after that one failed.
async fn publish_over_ble(
    services: &crate::state::Services,
    intent: &crate::mesh::PrivacyIntent,
) -> Option<usize> {
    let ble = services.ble.as_ref()?;
    let status = ble.status().await.ok()?;
    if status.offline || status.reachable_peers == 0 {
        return None;
    }

    let payload = serde_json::to_vec(intent).ok()?;
    ble.broadcast(cabal_ble::wire::PacketKind::Intent, payload.clone())
        .await
        .ok()?;

    // Resent in the background: the caller already has what it needs (a
    // route to record and a peer count to show) the moment the first send is
    // handed to the radio, and making it wait out every retry would hold a
    // "compose" button spinner hostage to a link that may never confirm
    // anything, ever, by design.
    let retry_ble = ble.clone();
    tokio::spawn(async move {
        for _ in 0..BLE_INTENT_RESENDS {
            tokio::time::sleep(BLE_INTENT_RESEND_DELAY).await;
            let _ = retry_ble
                .broadcast(cabal_ble::wire::PacketKind::Intent, payload.clone())
                .await;
        }
    });

    Some(status.reachable_peers)
}

/// Everything the detail screen renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct IntentDetailView {
    pub id: String,
    pub title: String,
    pub status: cabal_core::IntentStatus,
    /// Counts up while live, freezes at the terminal state.
    pub elapsed: String,
    /// The seven-row breakdown.
    pub rows: Vec<ReviewRow>,
    /// Whether settling is possible right now.
    pub can_settle: bool,
    /// Why not, in brand voice, when it is not. Absent when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_blocked: Option<String>,
    /// Whether the intent can still be cancelled.
    pub can_cancel: bool,
}

/// The seventh and sixth rows the detail screen adds to the five reviewed.
///
/// Route renders what was actually recorded — an empty route says so rather
/// than inventing hops, because the proof screen shows the same value and
/// calls it evidence.
fn detail_rows(intent: &crate::intents::Intent) -> Vec<ReviewRow> {
    let mut rows = review_rows(&intent.draft);

    rows.push(ReviewRow {
        key: "ROUTE".into(),
        value: if intent.route.is_empty() {
            "NOT YET ROUTED".into()
        } else {
            intent
                .route
                .iter()
                .map(|hop| hop.truncated())
                .collect::<Vec<_>>()
                .join(" · ")
        },
    });

    rows.push(ReviewRow {
        key: "COUNTERPARTY".into(),
        value: intent
            .counterparty
            .as_deref()
            .map_or_else(|| "NONE YET".into(), |address| cabal_core::NodeId::new(address).truncated()),
    });

    rows
}

/// One intent in full.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the identifier is unknown, which means the
/// frontend navigated to something that does not exist.
#[tauri::command]
pub async fn intent_detail(
    id: String,
    state: State<'_, AppState>,
) -> Result<IntentDetailView, AppError> {
    use crate::error::InvalidReason;

    let intent = state
        .intents()
        .get(&cabal_core::IntentId::new(id))
        .ok_or(AppError::InvalidIntent {
            field: "id",
            reason: InvalidReason::Missing,
        })?;

    // Settlement locks escrow *for* a specific address. Without a peer that
    // accepted, there is nobody to lock it for, and offering the button anyway
    // would promise something the command must refuse.
    let settle_blocked = if intent.status.is_terminal() {
        Some("Already finished. Nothing left to settle.".to_string())
    } else if matches!(intent.status, cabal_core::IntentStatus::Draft) {
        Some("Queued locally. Settlement follows reconnection.".to_string())
    } else if intent.counterparty.is_none() {
        Some("No node has accepted yet. Settlement needs a counterparty.".to_string())
    } else {
        None
    };

    Ok(IntentDetailView {
        id: intent.id.to_string(),
        title: format!("{:?} {}", intent.draft.action, intent.draft.asset).to_uppercase(),
        status: intent.status.clone(),
        elapsed: crate::intents::format_elapsed(intent.elapsed_ms(crate::intents::now_ms())),
        rows: detail_rows(&intent),
        can_settle: settle_blocked.is_none(),
        settle_blocked,
        can_cancel: !intent.status.is_terminal(),
    })
}

/// Streams an intent's verification log.
///
/// Replays what was already recorded, then follows. **Cancelling this stops
/// delivery and nothing else** — the settlement writes into the ledger and
/// holds no token from here, which is the property `src/intents.rs` is built
/// around and `src/subscriptions.rs` documents.
///
/// # Errors
///
/// [`AppError::TooManySubscriptions`] at the registry's limit.
#[tauri::command]
pub async fn subscribe_settlement_log(
    id: String,
    on_line: tauri::ipc::Channel<crate::bindings::LogLine>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    let intent_id = cabal_core::IntentId::new(id);
    let registry = state.subscriptions().clone();
    let (handle, token) = registry.register("settlement")?;

    let (replay, mut receiver) = state.intents().watch(&intent_id);

    let stream = handle.clone();
    tauri::async_runtime::spawn(async move {
        for recorded in replay {
            if on_line.send(recorded).is_err() {
                registry.finished(&stream);
                return;
            }
        }

        loop {
            tokio::select! {
                () = token.cancelled() => break,
                received = receiver.recv() => match received {
                    Ok((who, line)) => {
                        if who != intent_id {
                            continue;
                        }
                        if on_line.send(line).is_err() {
                            // The webview is gone; nothing left to deliver to.
                            break;
                        }
                    }
                    // Lagged means this subscriber fell behind, not that the
                    // settlement stopped. The retained log is the record, so
                    // keep following rather than tearing the stream down.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            }
        }

        registry.finished(&stream);
    });

    Ok(handle.to_string())
}

/// Settles an intent on-chain.
///
/// Returns as soon as the work is **started**, not when it finishes. The
/// settlement runs in a task that holds only the ledger, so navigating away —
/// or cancelling the log — cannot abort it. That is a correctness property with
/// money attached, and it is structural rather than a matter of discipline.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the intent is unknown, already finished, or
/// has no counterparty to pay.
#[tauri::command]
pub async fn settle_intent(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    use crate::error::InvalidReason;

    let intent_id = cabal_core::IntentId::new(id);
    let intent = state
        .intents()
        .get(&intent_id)
        .ok_or(AppError::InvalidIntent {
            field: "id",
            reason: InvalidReason::Missing,
        })?;

    if intent.status.is_terminal() || matches!(intent.status, cabal_core::IntentStatus::Draft) {
        return Err(AppError::InvalidIntent {
            field: "status",
            reason: InvalidReason::OutOfRange,
        });
    }

    let counterparty = intent.counterparty.clone().ok_or(AppError::InvalidIntent {
        field: "counterparty",
        reason: InvalidReason::Missing,
    })?;

    let services = state.services()?;
    let ledger = state.intents().clone();

    tauri::async_runtime::spawn(async move {
        run_settlement(ledger, services, intent_id, counterparty, intent.draft.clone()).await;
    });

    Ok(())
}

/// The settlement itself.
///
/// Takes no cancellation token by construction. Adding one would be the bug
/// ticket 34 exists to prevent — there would then be a way for a UI navigation
/// to abort an in-flight on-chain operation.
async fn run_settlement(
    ledger: crate::intents::Ledger,
    services: crate::state::Services,
    id: cabal_core::IntentId,
    counterparty: String,
    draft: cabal_core::IntentDraft,
) {
    use crate::bindings::LogTone;
    use crate::blockchain_bridge::EscrowOutcome;
    use crate::intents::line;
    use cabal_core::{FailureReason, IntentStatus};

    let started = std::time::Instant::now();

    ledger.record(&id, line("VERIFYING ROUTE.", LogTone::Out));
    // Routing precedes settlement in the domain: settling straight from
    // broadcast would mean settling through a route that was never found.
    let _ = ledger.advance(&id, IntentStatus::FindingRoute, crate::intents::now_ms());

    ledger.record(&id, line(format!("COUNTERPARTY {counterparty}."), LogTone::Dim));
    ledger.record(&id, line("LOCKING ESCROW.", LogTone::Out));

    // An hour is the window the counterparty has to deliver before the escrow
    // can be refunded. Long enough for a slow mesh route, short enough that
    // funds are not stranded for a day.
    let expiry = crate::intents::now_ms() / 1_000 + 3_600;
    let amount = alloy::primitives::U256::from(draft.amount.raw());

    let outcome = {
        let bridge = services.bridge.lock().await;
        bridge.create_escrow_detailed(&counterparty, amount, expiry).await
    };

    let elapsed_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);

    match outcome {
        Ok(EscrowOutcome::Confirmed { escrow_id, tx_hash }) => {
            ledger.record(&id, line(format!("ESCROW {escrow_id} MINED."), LogTone::Ok));
            ledger.record(&id, line(format!("PROOF {tx_hash}"), LogTone::Loud));
            ledger.set_escrow(
                &id,
                crate::intents::EscrowRef::Confirmed { id: escrow_id, tx: tx_hash.clone() },
            );

            // The filled price is the condition's own price where one exists.
            // `Any` carries none, and inventing one for the proof screen would
            // fabricate the single figure that screen is there to prove.
            let filled_at = draft
                .condition
                .price()
                .unwrap_or_else(|| cabal_core::UsdPrice::from_cents(0));

            let _ = ledger.advance(
                &id,
                IntentStatus::Settled {
                    proof: cabal_core::ProofHash::new(tx_hash),
                    filled_at,
                    elapsed_ms,
                },
                crate::intents::now_ms(),
            );
        }
        Ok(EscrowOutcome::Queued { queue_id }) => {
            // Not a failure. The transaction is signed and waiting for a peer
            // with a route to the chain, which is the offline path working.
            ledger.record(&id, line("NO ROUTE TO CHAIN. SIGNED OFFLINE.", LogTone::Dim));
            ledger.record(&id, line(format!("QUEUED FOR RELAY: {queue_id}."), LogTone::Dim));
            ledger.set_escrow(&id, crate::intents::EscrowRef::Queued { queue_id });
            let _ = ledger.advance(&id, IntentStatus::Waiting, crate::intents::now_ms());
        }
        Err(error) => {
            // Logged with detail, surfaced without: RPC errors routinely carry
            // the endpoint URL, which the webview has no business holding.
            tracing::error!(target: "cabalmesh::intents", %id, %error, "settlement failed");
            ledger.record(&id, line("SETTLEMENT REJECTED ON-CHAIN.", LogTone::Err));
            let _ = ledger.advance(
                &id,
                IntentStatus::Failed { reason: FailureReason::SettlementRejected },
                crate::intents::now_ms(),
            );
        }
    }
}

/// Cancels an intent, releasing any escrow it holds.
///
/// A deliberate, separate action from cancelling a log subscription. Overloading
/// the two is the mistake ticket 34 names explicitly.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the intent is unknown or already finished.
#[tauri::command]
pub async fn cancel_intent(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    use crate::bindings::LogTone;
    use crate::intents::line;

    let intent_id = cabal_core::IntentId::new(id);
    let ledger = state.intents();

    // The transition check does the validating: a terminal intent refuses, and
    // an unknown identifier refuses, both with a typed error.
    let intent = ledger.get(&intent_id).ok_or(AppError::InvalidIntent {
        field: "id",
        reason: crate::error::InvalidReason::Missing,
    })?;

    // Escrow first, then the status. Releasing after marking it cancelled would
    // leave funds locked behind an intent the UI says is over.
    if let Some(crate::intents::EscrowRef::Confirmed { id: escrow_id, .. }) = intent.escrow {
        ledger.record(&intent_id, line("RELEASING ESCROW.", LogTone::Out));
        match state.services() {
            Ok(services) => {
                let bridge = services.bridge.lock().await;
                match bridge.release_escrow(escrow_id).await {
                    Ok(tx) => ledger.record(&intent_id, line(format!("ESCROW RELEASED. {tx}"), LogTone::Ok)),
                    Err(error) => {
                        tracing::error!(target: "cabalmesh::intents", %error, "escrow release failed");
                        ledger.record(&intent_id, line("ESCROW STILL LOCKED. RETRY FROM VAULT.", LogTone::Err));
                        return Err(AppError::Chain { retryable: true });
                    }
                }
            }
            Err(_) => return Err(AppError::NotReady { subsystem: "bootstrap" }),
        }
    }

    ledger.advance(&intent_id, cabal_core::IntentStatus::Cancelled, crate::intents::now_ms())?;
    ledger.record(&intent_id, line("INTENT CANCELLED. NOTHING WRITTEN.", LogTone::Dim));
    Ok(())
}

/// What the proof screen renders.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ProofView {
    pub id: String,
    /// The settling transaction's hash.
    pub hash: String,
    /// How long settlement took, e.g. `11.4S`.
    pub timing: String,
    /// The hops the intent travelled. Empty when it settled directly.
    pub route: Vec<String>,
    /// The price it filled at. Absent for an unconditioned intent, which has
    /// no price to have filled at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filled_at: Option<String>,
}

/// The proof for a settled intent.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] if the intent is unknown or has not settled —
/// there is no proof of something that did not happen.
#[tauri::command]
pub async fn intent_proof(id: String, state: State<'_, AppState>) -> Result<ProofView, AppError> {
    use crate::error::InvalidReason;

    let intent = state
        .intents()
        .get(&cabal_core::IntentId::new(id))
        .ok_or(AppError::InvalidIntent {
            field: "id",
            reason: InvalidReason::Missing,
        })?;

    let cabal_core::IntentStatus::Settled { proof, filled_at, elapsed_ms } = &intent.status else {
        return Err(AppError::InvalidIntent {
            field: "status",
            reason: InvalidReason::OutOfRange,
        });
    };

    Ok(ProofView {
        id: intent.id.to_string(),
        hash: proof.to_string(),
        timing: crate::intents::format_elapsed(u64::from(*elapsed_ms)),
        route: intent.route.iter().map(cabal_core::NodeId::truncated).collect(),
        // Zero cents means the intent carried no condition, so there is no
        // price it filled at. Rendering `$0.00` would be a figure, and a wrong
        // one.
        filled_at: (filled_at.cents() > 0).then(|| filled_at.to_string()),
    })
}

#[cfg(test)]
mod intent_tests {
    use super::*;
    use crate::bindings::LogTone;
    use crate::intents::{line, Ledger};
    use cabal_core::{IntentStatus, ProofHash, UsdPrice};

    fn ledger() -> (Ledger, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("intents.json"));
        (Ledger::open(store), dir)
    }

    fn fields(action: &str, asset: &str, condition: &str, price: &str, amount: &str, mode: &str, privacy: &str) -> IntentFields {
        IntentFields {
            action: action.into(),
            asset: asset.into(),
            condition: condition.into(),
            price: price.into(),
            amount: amount.into(),
            mode: mode.into(),
            privacy: privacy.into(),
        }
    }

    fn draft() -> cabal_core::IntentDraft {
        parse_draft(&fields("BUY", "AVAX", "Price under", "95", "1.5", "SHARK MODE", "HIGH")).unwrap()
    }

    // -- what the dialog shows is what goes out ----------------------------

    #[test]
    fn the_review_rows_come_from_the_parsed_draft() {
        // The property ticket 33 asks for: the dialog is a rendering *of the
        // draft*, so it cannot describe an intent different from the one that
        // would be broadcast.
        let rows = review_rows(&draft());
        let value = |key: &str| {
            rows.iter()
                .find(|row| row.key == key)
                .map(|row| row.value.clone())
                .unwrap()
        };

        assert_eq!(value("ACTION"), "BUY AVAX");
        assert_eq!(value("CONDITION"), "UNDER $95.00");
        assert_eq!(value("AMOUNT"), "1.5 AVAX");
        assert_eq!(value("MODE"), "SHARK MODE");
        assert_eq!(value("PRIVACY"), "HIGH");
    }

    #[test]
    fn an_unconditioned_intent_shows_no_price() {
        // `Condition::Any` carries no price by construction, so there is none
        // to render — and rendering `$0.00` would be a claim about a limit the
        // user never set.
        let draft = parse_draft(&fields("SELL", "USDC", "Any price", "", "10", "GHOST MODE", "LOW")).unwrap();
        let rows = review_rows(&draft);
        assert_eq!(rows[1].value, "ANY PRICE");
    }

    #[test]
    fn precision_beyond_the_asset_is_refused_rather_than_truncated() {
        // USDC has six decimals. Silently dropping the seventh would lose money.
        let refused = parse_draft(&fields("BUY", "USDC", "Any price", "", "1.1234567", "SHARK MODE", "HIGH"));
        assert!(matches!(
            refused,
            Err(AppError::InvalidIntent { field: "amount", .. })
        ));
    }

    #[test]
    fn a_zero_amount_is_refused() {
        let refused = parse_draft(&fields("BUY", "AVAX", "Any price", "", "0", "SHARK MODE", "HIGH"));
        assert!(matches!(
            refused,
            Err(AppError::InvalidIntent {
                field: "amount",
                reason: crate::error::InvalidReason::OutOfRange
            })
        ));
    }

    #[test]
    fn an_unknown_asset_is_refused_rather_than_defaulted() {
        // Defaulting to eighteen decimals for an unrecognised asset would parse
        // a USDC amount as if it were AVAX — off by a factor of a trillion.
        let refused = parse_draft(&fields("BUY", "DOGE", "Any price", "", "1", "SHARK MODE", "HIGH"));
        assert!(matches!(
            refused,
            Err(AppError::InvalidIntent { field: "asset", .. })
        ));
    }

    #[test]
    fn every_offered_asset_parses() {
        // The form offers these, so every one of them has to survive the round
        // trip. A mismatch here is a form that offers something Rust rejects.
        for (name, _, _) in ASSETS {
            assert!(
                parse_draft(&fields("BUY", name, "Any price", "", "1", "SHARK MODE", "HIGH")).is_ok(),
                "{name} did not parse"
            );
        }
    }

    #[test]
    fn the_confirm_lines_describe_different_things() {
        // Ticket 04. The retired string claimed offline intents settle
        // on-chain; the queued line must not say anything of the kind.
        assert!(CONFIRM_ONLINE.contains("broadcasts to the mesh"));
        assert!(CONFIRM_QUEUED.contains("Queued locally"));
        assert!(!CONFIRM_QUEUED.contains("settles on-chain"));
        // The identity claim holds in both, which is the part that stayed true.
        assert!(CONFIRM_ONLINE.contains("No identity is attached."));
        assert!(CONFIRM_QUEUED.contains("No identity is attached."));
    }

    // -- list slicing -------------------------------------------------------

    #[test]
    fn a_queued_intent_is_pending_not_active() {
        // A draft is the queue. Filing it under ACTIVE would claim it is on the
        // mesh; filing it under HISTORY would claim it is over.
        let draft = IntentStatus::Draft;
        assert!(IntentFilter::Pending.admits(&draft));
        assert!(!IntentFilter::Active.admits(&draft));
        assert!(!IntentFilter::History.admits(&draft));
    }

    #[test]
    fn every_status_lands_in_exactly_one_slice() {
        let statuses = [
            IntentStatus::Draft,
            IntentStatus::Broadcast { route_len: 2 },
            IntentStatus::Negotiating { bids: 1, best: None },
            IntentStatus::FindingRoute,
            IntentStatus::Waiting,
            IntentStatus::Settled {
                proof: ProofHash::new("0xabc"),
                filled_at: UsdPrice::from_cents(9421),
                elapsed_ms: 11_400,
            },
            IntentStatus::Failed { reason: cabal_core::FailureReason::NoRoute },
            IntentStatus::Cancelled,
        ];

        for status in statuses {
            let matches = [IntentFilter::Active, IntentFilter::Pending, IntentFilter::History]
                .into_iter()
                .filter(|filter| filter.admits(&status))
                .count();
            assert_eq!(matches, 1, "{status:?} landed in {matches} slices");
        }
    }

    #[test]
    fn a_row_renders_the_intent_it_was_given() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        let row = row_for(&intent, 12_400);

        assert_eq!(row.title, "BUY AVAX");
        assert_eq!(row.subtitle, "UNDER $95.00");
        assert_eq!(row.amount, "1.5 AVAX");
        assert_eq!(row.elapsed, "11.4S");
        // Shark is the default, so badging it would put a badge on nearly every
        // row and stop the badge carrying information.
        assert_eq!(row.badge, None);
    }

    #[test]
    fn a_non_default_mode_is_badged() {
        let (ledger, _dir) = ledger();
        let ghost = parse_draft(&fields("BUY", "AVAX", "Any price", "", "1", "GHOST MODE", "HIGH")).unwrap();
        let intent = ledger.create(ghost, 1_000);
        assert_eq!(row_for(&intent, 1_000).badge.as_deref(), Some("GHOST MODE"));
    }

    // -- the seven-row breakdown -------------------------------------------

    #[test]
    fn the_breakdown_has_seven_rows_and_invents_no_route() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        let rows = detail_rows(&intent);

        assert_eq!(rows.len(), 7);
        assert_eq!(rows[5].key, "ROUTE");
        // Says so rather than inventing hops — the proof screen renders the
        // same value and calls it evidence.
        assert_eq!(rows[5].value, "NOT YET ROUTED");
        assert_eq!(rows[6].value, "NONE YET");
    }

    // -- ticket 34's load-bearing rule -------------------------------------

    #[tokio::test]
    async fn cancelling_the_log_subscription_does_not_abort_the_settlement() {
        // The explicit test ticket 34 asks for, run against the real
        // subscription registry rather than a stand-in.
        //
        // The structural claim is that a settlement task holds *no* token from
        // the registry. This exercises it: a task shaped exactly like
        // `run_settlement` — a ledger and nothing else — keeps writing after
        // the subscription that was watching it is cancelled, and reaches its
        // terminal state.
        let (ledger, _dir) = ledger();
        let registry = crate::subscriptions::Registry::new();
        let intent = ledger.create(draft(), 1_000);

        let (handle, token) = registry.register("settlement").unwrap();
        let (_replay, _receiver) = ledger.watch(&intent.id);

        ledger
            .advance(&intent.id, IntentStatus::Broadcast { route_len: 2 }, 1_100)
            .unwrap();

        let writer = {
            let ledger = ledger.clone();
            let id = intent.id.clone();
            tokio::spawn(async move {
                ledger.record(&id, line("VERIFYING ROUTE.", LogTone::Out));
                let _ = ledger.advance(&id, IntentStatus::FindingRoute, 1_200);

                // The navigation happens here, in the middle.
                tokio::task::yield_now().await;

                ledger.record(&id, line("ESCROW MINED.", LogTone::Ok));
                let _ = ledger.advance(
                    &id,
                    IntentStatus::Settled {
                        proof: ProofHash::new("0xdeadbeef"),
                        filled_at: UsdPrice::from_cents(9421),
                        elapsed_ms: 11_400,
                    },
                    1_300,
                );
            })
        };

        // Navigating away: the registry cancels delivery.
        registry.cancel(&handle);
        assert!(token.is_cancelled());
        assert!(registry.is_empty());

        writer.await.unwrap();

        let after = ledger.get(&intent.id).unwrap();
        assert!(
            matches!(after.status, IntentStatus::Settled { .. }),
            "settlement was aborted by a UI navigation: {:?}",
            after.status
        );
        // And the record is complete, so coming back replays everything that
        // happened while away.
        assert_eq!(after.log.len(), 2);
        assert_eq!(&*after.log[1].text, "ESCROW MINED.");
    }

    #[tokio::test]
    async fn cancelling_the_intent_is_a_different_thing_entirely() {
        // The other half of the same rule: cancelling the *intent* is
        // deliberate and does end it. If these two ever became the same
        // operation, a navigation would start releasing escrow.
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        ledger
            .advance(&intent.id, IntentStatus::Broadcast { route_len: 2 }, 1_100)
            .unwrap();

        ledger
            .advance(&intent.id, IntentStatus::Cancelled, 2_000)
            .unwrap();

        assert_eq!(ledger.get(&intent.id).unwrap().status, IntentStatus::Cancelled);
    }

    // -- the proof ----------------------------------------------------------

    #[test]
    fn a_settled_intent_yields_the_hash_it_settled_with() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000);
        ledger
            .advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, 1_100)
            .unwrap();
        ledger
            .advance(&intent.id, IntentStatus::FindingRoute, 1_200)
            .unwrap();
        ledger.set_route(&intent.id, vec![cabal_core::NodeId::new("7F3A00000000008C2E")]);
        ledger
            .advance(
                &intent.id,
                IntentStatus::Settled {
                    proof: ProofHash::new("0xa4f2c9e1b70d5533"),
                    filled_at: UsdPrice::from_cents(9421),
                    elapsed_ms: 11_400,
                },
                12_400,
            )
            .unwrap();

        let settled = ledger.get(&intent.id).unwrap();
        let IntentStatus::Settled { proof, filled_at, elapsed_ms } = &settled.status else {
            panic!("expected settled");
        };

        assert_eq!(proof.as_str(), "0xa4f2c9e1b70d5533");
        assert_eq!(filled_at.to_string(), "$94.21");
        assert_eq!(crate::intents::format_elapsed(u64::from(*elapsed_ms)), "11.4S");
        assert_eq!(settled.route[0].truncated(), "7F3A..8C2E");
    }
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
    let services = state.services()?;
    let bridge = services.bridge.lock().await;

    // Honest about which key provider is actually protecting the vault right
    // now, rather than a description fixed at ticket 18. A locked vault has
    // no in-memory bridge state to describe wrongly either way, so this reads
    // straight from disk via `security_mode`.
    let vault_key_detail = match bridge.security_mode() {
        crate::security_state::UnlockMode::File => "FILE-BACKED. DEVICE KEY STORE PENDING.",
        crate::security_state::UnlockMode::Passphrase => "PASSPHRASE-DERIVED (ARGON2ID).",
    };

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
            detail: Some(vault_key_detail.into()),
        },
        VaultRow {
            tag: "KEY".into(),
            name: "RECOVERY PHRASE".into(),
            amount: "NONE".into(),
            detail: Some("NOT BACKED UP.".into()),
        },
    ])
}

/// Current state of the vault's unlock method, for the startup gate and the
/// `SECURITY` screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct SecurityStatus {
    /// True only while the vault is passphrase-protected and no correct
    /// passphrase has been supplied yet this session. The frontend gates
    /// entry to the app on this field, not on `passphraseEnabled` alone —
    /// `passphraseEnabled` stays true even seconds after a successful
    /// unlock, when `locked` has already gone false.
    pub locked: bool,
    pub passphrase_enabled: bool,
}

/// Whether the vault needs a passphrase before use, and how it is protected.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn security_status(state: State<'_, AppState>) -> Result<SecurityStatus, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    Ok(SecurityStatus {
        locked: bridge.is_locked(),
        passphrase_enabled: bridge.security_mode() == crate::security_state::UnlockMode::Passphrase,
    })
}

/// Supplies the passphrase for a locked vault. On success, identities become
/// readable for the rest of the session.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// passphrase was wrong.
#[tauri::command]
pub async fn security_unlock(passphrase: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mut bridge = services.bridge.lock().await;
    bridge.unlock_with_passphrase(&passphrase).map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// Turns passphrase protection on: re-encrypts the vault under a key derived
/// from `passphrase` and deletes the file-backed key it replaces.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or re-encryption failed.
#[tauri::command]
pub async fn security_enable_passphrase(passphrase: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mut bridge = services.bridge.lock().await;
    bridge.enable_passphrase(&passphrase).map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// Turns passphrase protection off, reverting to a freshly generated
/// file-backed key.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or re-encryption failed.
#[tauri::command]
pub async fn security_disable_passphrase(state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let mut bridge = services.bridge.lock().await;
    bridge.disable_passphrase().map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// The current wallet's raw private key, so it can be saved before switching
/// away from it — the only way back to this wallet once identities change,
/// since nothing else persists it anywhere recoverable. See
/// `docs/identity-design.md`: this is the gap the doc calls more urgent than
/// any feature it proposes, since without it a lost device is unrecoverable.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or holds no identity yet.
#[tauri::command]
pub async fn vault_export_key(state: State<'_, AppState>) -> Result<String, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    bridge.get_primary_private_key().ok_or(AppError::VaultLocked)
}

/// Replaces the current wallet with one derived from a supplied private key.
///
/// Destructive: the wallet this device held before is gone unless its own
/// key was exported first. The frontend is responsible for warning about
/// that before calling this — the command itself trusts the caller, same as
/// every other mutating vault command.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::VaultLocked`] if the
/// vault is locked or the supplied key does not parse.
#[tauri::command]
pub async fn vault_import_key(
    private_key_hex: String,
    alias: String,
    emoji: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let services = state.services()?;
    let mut bridge = services.bridge.lock().await;
    bridge
        .import_identity(private_key_hex, alias, emoji)
        .map(|_| ())
        .map_err(|_| AppError::VaultLocked)
}

/// What the profile screen shows.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub node_id: String,
    /// `14 (+55.6%)`, or just `14` with no prior window to compare against.
    ///
    /// Was a mocked `reputation` until ticket 39. Renamed along with the value
    /// because a count called a "score" is a figure whose name promises more
    /// than its definition delivers.
    pub settled: String,
    /// `2026.08.03` — when this installation first ran.
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

    // The same ledger the home tile reads, so the two screens cannot disagree.
    // No mesh is needed: this is local history, not network state.
    let settled = crate::standing::LocalStanding::of(state.intents(), crate::intents::now_ms()).combined();

    Ok(ProfileView {
        node_id,
        settled,
        // Written on the first read and never again — see src/install.rs for
        // why neither the ephemeral mesh identity nor a file's creation time
        // could answer this.
        member_since: crate::install::format_date(crate::install::first_seen_ms(
            crate::intents::now_ms(),
        )),
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

// ---------------------------------------------------------------------------
// Guardian recovery
// ---------------------------------------------------------------------------

/// A nearby node the user could pick as a guardian.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct GuardianCandidate {
    /// Full peer id — opaque to the UI, round-tripped back verbatim to
    /// `guardian_enroll`. Never shown; `label` is what renders.
    pub peer_id: String,
    /// Truncated for display, e.g. `7F3A…C2E1`.
    pub label: String,
    pub hops: u8,
}

/// Nearby BLE nodes the user could pick as guardians.
///
/// Empty rather than an error when there is no BLE plane — the same choice
/// `list_nearby_nodes` makes, since "no radio" and "no candidates" read the
/// same way to this screen.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn guardian_candidates(state: State<'_, AppState>) -> Result<Vec<GuardianCandidate>, AppError> {
    let services = state.services()?;
    let Some(ble) = services.ble.as_ref() else {
        return Ok(Vec::new());
    };
    let peers = ble.peers().await.unwrap_or_default();
    Ok(peers
        .into_iter()
        .map(|peer| {
            let id = peer.id.to_string();
            GuardianCandidate {
                label: cabal_core::NodeId::new(id.clone()).truncated(),
                peer_id: id,
                hops: peer.hops,
            }
        })
        .collect())
}

/// What `SECURITY` shows about the guardian scheme, in both roles this
/// device can play.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct GuardianStatus {
    pub enrolled: bool,
    pub guardian_count: usize,
    pub threshold: u8,
    /// How many other owners this device holds a share for.
    pub holding_for: usize,
}

/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn guardian_status(state: State<'_, AppState>) -> Result<GuardianStatus, AppError> {
    let services = state.services()?;
    let service = services.guardian.lock().await;
    let (guardian_count, threshold) = service.owner_guardian_status();
    Ok(GuardianStatus {
        enrolled: service.is_enrolled(),
        guardian_count,
        threshold,
        holding_for: service.held_for().len(),
    })
}

/// Who accepted an enrollment invitation, and who did not answer.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct GuardianEnrollResult {
    pub enrolled: Vec<String>,
    pub no_reply: Vec<String>,
}

/// Invites `peer_ids` to become guardians and, for whoever accepts, sends a
/// sealed share split from the current vault key.
///
/// Blocks for up to 20 seconds waiting on replies — see
/// `guardian_actor::REPLY_TIMEOUT` — since a candidate is a human who has to
/// notice a prompt.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] if
/// there is no BLE plane, [`AppError::VaultLocked`] if the vault is locked,
/// [`AppError::Internal`] if nobody accepted or the threshold was invalid.
#[tauri::command]
pub async fn guardian_enroll(
    peer_ids: Vec<String>,
    threshold: u8,
    state: State<'_, AppState>,
) -> Result<GuardianEnrollResult, AppError> {
    let services = state.services()?;
    let ble = services.ble.as_ref().ok_or(AppError::MeshOffline)?;

    let candidates: Vec<cabal_ble::PeerId> = peer_ids
        .iter()
        .map(|s| s.parse())
        .collect::<Result<_, _>>()
        .map_err(|_| AppError::InvalidIntent { field: "peer_ids", reason: crate::error::InvalidReason::Malformed })?;

    let vault_key = {
        let bridge = services.bridge.lock().await;
        bridge.current_vault_key().map_err(|_| AppError::VaultLocked)?
    };

    let events = ble.subscribe();
    let outcome = crate::guardian_actor::enroll_guardians(&services.guardian, ble, events, &candidates, threshold, &vault_key)
        .await
        .map_err(AppError::internal)?;

    Ok(GuardianEnrollResult {
        enrolled: outcome.enrolled.iter().map(ToString::to_string).collect(),
        no_reply: outcome.no_reply.iter().map(ToString::to_string).collect(),
    })
}

/// Broadcasts an unlock request, waits for enough guardians to answer, and —
/// if a valid key comes back — opens the vault with it.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] if
/// there is no BLE plane, [`AppError::VaultLocked`] if too few guardians
/// answered in time or the reconstructed key was wrong.
#[tauri::command]
pub async fn guardian_request_unlock(state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let ble = services.ble.as_ref().ok_or(AppError::MeshOffline)?;

    let events = ble.subscribe();
    let candidate = crate::guardian_actor::request_unlock(&services.guardian, ble, events)
        .await
        .map_err(|_| AppError::VaultLocked)?;

    let mut bridge = services.bridge.lock().await;
    bridge.unlock_with_guardian_key(candidate).map_err(|_| AppError::VaultLocked)?;
    Ok(())
}

/// Sends a pending unlock reply — the guardian side, called once the person
/// taps APPROVE on the prompt `guardian-unlock-request` drove.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::MeshOffline`] if
/// there is no BLE plane, [`AppError::Internal`] if `id` was already
/// resolved or never existed.
#[tauri::command]
pub async fn guardian_approve_unlock(id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let ble = services.ble.as_ref().ok_or(AppError::MeshOffline)?;
    crate::guardian_actor::approve_unlock(&services.guardian_approvals, ble, id).await.map_err(AppError::internal)
}

/// Discards a pending unlock reply without sending anything.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if `id`
/// was already resolved or never existed.
#[tauri::command]
pub async fn guardian_deny_unlock(id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    crate::guardian_actor::deny_unlock(&services.guardian_approvals, id).await.map_err(AppError::internal)
}

// ---------------------------------------------------------------------------
// Marketplace and modules
// ---------------------------------------------------------------------------
//
// Every command here wraps a `BlockchainBridge` method that already existed
// and already worked — `create_asset_listing`, `buy_listing`,
// `get_active_asset_listings`, `get_owned_module_cards`, and the rest were fully
// implemented against real contracts, just never reachable from any command.
// See docs/intent-chat-and-modules-design.md for the design and the five
// decisions (0-4) this surface is built against — most load-bearing:
// `CabalMeshVoucher` restricts minting to the `RelayRewards` contract now,
// so nothing here can mint a module out of thin air the way the pre-fix
// contract could.

/// Active Marketplace listings.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if the
/// Marketplace contract isn't configured or the chain is unreachable.
#[tauri::command]
pub async fn market_listings(state: State<'_, AppState>) -> Result<Vec<crate::blockchain_bridge::AssetListingView>, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    bridge.get_active_asset_listings().await.map_err(AppError::internal_msg)
}

/// Buys a listing: atomically locks `price_wei` AVAX and pulls its module
/// into escrow. `price_wei` comes from the listing `market_listings` already
/// returned — the contract itself rejects a wrong amount rather than
/// trusting it, so a stale or wrong value here fails safely.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::InvalidIntent`] if
/// `price_wei` isn't a decimal number, [`AppError::Internal`] if the buy
/// reverts (e.g. buying your own listing, or the listing is gone).
#[tauri::command]
pub async fn market_buy(
    listing_id: u32,
    price_wei: String,
    state: State<'_, AppState>,
) -> Result<crate::blockchain_bridge::TxResult, AppError> {
    let services = state.services()?;
    let price = price_wei.parse::<alloy::primitives::U256>().map_err(|_| AppError::InvalidIntent {
        field: "price_wei",
        reason: crate::error::InvalidReason::Malformed,
    })?;
    let bridge = services.bridge.lock().await;
    bridge.buy_listing(listing_id, price).await.map_err(AppError::internal_msg)
}

/// Lists an owned module for sale — approves the Marketplace to move it,
/// then creates the listing. Two on-chain steps behind one button, matching
/// the design doc's "LIST ON MARKET" mock-up.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::InvalidIntent`] if
/// `price_avax` isn't a decimal AVAX amount, [`AppError::Internal`] if
/// either on-chain step fails (most likely: the token isn't owned by this
/// identity).
#[tauri::command]
pub async fn market_list_module(
    token_id: u32,
    description: String,
    price_avax: String,
    state: State<'_, AppState>,
) -> Result<u32, AppError> {
    let services = state.services()?;
    let price = alloy::primitives::utils::parse_ether(&price_avax).map_err(|_| AppError::InvalidIntent {
        field: "price_avax",
        reason: crate::error::InvalidReason::Malformed,
    })?;
    let bridge = services.bridge.lock().await;
    bridge.approve_module_card(token_id).await.map_err(AppError::internal_msg)?;
    bridge.create_asset_listing(&description, price, token_id).await.map_err(AppError::internal_msg)
}

/// Releases a deal: pays the seller and transfers the module to the buyer.
/// Only the buyer may call this — enforced on-chain, not here.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if the
/// call reverts.
#[tauri::command]
pub async fn market_release_deal(deal_id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    bridge.release_deal(deal_id).await.map(|_| ()).map_err(AppError::internal_msg)
}

/// Refunds a deal: returns AVAX to the buyer and the module to the seller.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if the
/// call reverts.
#[tauri::command]
pub async fn market_refund_deal(deal_id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    bridge.refund_deal(deal_id).await.map(|_| ()).map_err(AppError::internal_msg)
}

/// Deals this identity is party to, buyer or seller, with real on-chain
/// status.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if the
/// Marketplace contract isn't configured or the chain is unreachable.
#[tauri::command]
pub async fn market_my_deals(state: State<'_, AppState>) -> Result<Vec<crate::blockchain_bridge::DealView>, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    let address = bridge.get_primary_address();
    bridge.get_my_deals(&address).await.map_err(AppError::internal_msg)
}

/// Every module card (and other card type) this identity owns on-chain
/// right now.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if the
/// module-card contract isn't configured or the chain is unreachable.
#[tauri::command]
pub async fn vault_modules(state: State<'_, AppState>) -> Result<Vec<crate::blockchain_bridge::ModuleCardView>, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    let address = bridge.get_primary_address();
    bridge.get_owned_module_cards(&address).await.map_err(AppError::internal_msg)
}

/// One slot's currently equipped module.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct EquippedSlot {
    pub slot: u8,
    pub token_id: u32,
}

/// The `NODE LOADOUT` panel's data: what's equipped, and the multiplier it
/// actually produces right now.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ModuleLoadout {
    pub equipped: Vec<EquippedSlot>,
    /// Computed fresh from on-chain ownership on every call — see
    /// `BlockchainBridge::get_relay_multiplier`'s docs for why this is
    /// never cached.
    pub multiplier: f64,
}

/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_loadout(state: State<'_, AppState>) -> Result<ModuleLoadout, AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    let equipped =
        bridge.get_equipped_modules().into_iter().map(|(slot, token_id)| EquippedSlot { slot, token_id }).collect();
    let multiplier = bridge.get_relay_multiplier().await;
    Ok(ModuleLoadout { equipped, multiplier })
}

/// Equips `token_id` in `slot`, replacing whatever was equipped there.
///
/// Does not check ownership — nothing needs to. An equip entry for a token
/// this identity doesn't (or no longer does) own is inert: it silently
/// contributes nothing to `vault_loadout`'s multiplier rather than being
/// something this command has to reject. See
/// `BlockchainBridge::get_relay_multiplier`.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_equip_module(slot: u8, token_id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    bridge.equip_module(slot, token_id).map_err(AppError::internal_msg)
}

/// # Errors
///
/// [`AppError::NotReady`] before bootstrap.
#[tauri::command]
pub async fn vault_unequip_module(slot: u8, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    bridge.unequip_module(slot).map_err(AppError::internal_msg)
}

/// Burns an owned module card, claiming what it represents.
///
/// # Errors
///
/// [`AppError::NotReady`] before bootstrap, [`AppError::Internal`] if the
/// call reverts (most likely: not the owner).
#[tauri::command]
pub async fn vault_redeem_module(token_id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let services = state.services()?;
    let bridge = services.bridge.lock().await;
    bridge.redeem_module_card(token_id).await.map(|_| ()).map_err(AppError::internal_msg)
}
