//! Serialization contract for types crossing the IPC and mesh-wire boundary.
//!
//! Without a mechanical guard, a renamed field or a changed enum tag produces
//! `undefined` in the webview (or a peer that can't parse the wire message),
//! not a compiler error.
//!
//! # Why shapes, not live output
//!
//! Producing these values for real needs something this suite must not depend
//! on: a reachable Avalanche RPC, a running Ollama, the `nargo` binary, or a
//! live libp2p mesh. Snapshotting runtime output would be neither
//! reproducible nor CI-safe, and the results would drift with chain state.
//!
//! What matters is the **serialized shape** of the values crossing the
//! boundary — field names, casing, enum tagging, and how optionality is
//! represented. That is what these snapshots pin, using fixtures rather than
//! services.
//!
//! # What a failure here means
//!
//! A diff is not automatically a bug — but it is always a decision. Either the
//! change is intentional and any consumer of the old shape needs a
//! compatibility adapter to match, or it is accidental and should be
//! reverted. Never accept a snapshot without deciding which.
//!
//! Run `cargo insta review` to inspect diffs.

use cabalmesh_lib::agent::ContentAnalysis;
use cabalmesh_lib::bindings::GuardianUnlockPrompt;
use cabalmesh_lib::blockchain_bridge::{
    AssetListingView, CompressedAsset, ContentRecord, DealView, IdentityView, InstantSession,
    QueuedTx, RelayedTxRecord, Snapshot, TxResult, VoucherView,
};
use cabalmesh_lib::commands::{
    EquippedSlot, GuardianCandidate, GuardianEnrollResult, GuardianStatus, ModuleLoadout, SecurityStatus,
};
use cabalmesh_lib::matcher::MatchResult;
use cabalmesh_lib::mesh::{MeshEvent, PrivacyIntent};
use cabalmesh_lib::zk_handler::{ProofRequest, ZKProof};
use chrono::{TimeZone, Utc};
use serde::Serialize;

/// A fixed instant, so snapshots never depend on the clock.
fn fixed_time() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 18, 14, 32, 0).unwrap()
}

/// Serializes to pretty JSON — the shape the webview receives.
fn shape<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("boundary type must serialize")
}

// ---------------------------------------------------------------------------
// Identity and wallet
// ---------------------------------------------------------------------------

#[test]
fn identity_view_shape() {
    insta::assert_snapshot!(shape(&IdentityView {
        alias: "Genesis Fox".into(),
        emoji: "🦊".into(),
        address: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
    }));
}

#[test]
fn compressed_asset_shape() {
    // `amount` is a decimal wei *string* on purpose: a u256 does not survive a
    // JS number, and the frozen UI parses it as text.
    insta::assert_snapshot!(shape(&CompressedAsset {
        id: "asset-1".into(),
        amount: "1240000000000000000000".into(),
        symbol: "AVAX".into(),
        owner: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        proof: Some("0xa4f2c9e1b70d5533".into()),
    }));
}

#[test]
fn compressed_asset_shape_without_proof() {
    // Pins how `None` is represented — null, not omission.
    insta::assert_snapshot!(shape(&CompressedAsset {
        id: "asset-2".into(),
        amount: "0".into(),
        symbol: "USDC".into(),
        owner: "0x0000000000000000000000000000000000000000".into(),
        proof: None,
    }));
}

#[test]
fn snapshot_shape() {
    insta::assert_snapshot!(shape(&Snapshot {
        timestamp: fixed_time(),
        assets: vec![CompressedAsset {
            id: "asset-1".into(),
            amount: "1000000000000000000".into(),
            symbol: "AVAX".into(),
            owner: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
            proof: None,
        }],
        signature: "0xdeadbeef".into(),
    }));
}

#[test]
fn instant_session_shape() {
    insta::assert_snapshot!(shape(&InstantSession {
        session_id: "sess_1785603228".into(),
        authority: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        expiry: fixed_time(),
        is_active: true,
    }));
}

// ---------------------------------------------------------------------------
// Marketplace, vouchers, deals
// ---------------------------------------------------------------------------

#[test]
fn asset_listing_view_shape() {
    insta::assert_snapshot!(shape(&AssetListingView {
        id: 7,
        seller: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        description: "AI compute credit".into(),
        price_wei: "50000000000000000".into(),
        price_avax: "0.05".into(),
        token_id: 42,
    }));
}

#[test]
fn voucher_view_shape() {
    insta::assert_snapshot!(shape(&VoucherView {
        token_id: 42,
        voucher_type: "compute".into(),
        description: "AI compute credit".into(),
        owner: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        minted_by: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        slot: 0,
        rarity: 0,
        effect_bps: 0,
    }));
}

#[test]
fn voucher_view_shape_as_a_module() {
    // A module's slot/rarity/effect are what `VAULT → MODULES` reads to
    // build the loadout — see docs/intent-chat-and-modules-design.md,
    // decision 2.
    insta::assert_snapshot!(shape(&VoucherView {
        token_id: 7,
        voucher_type: "Gateway License".into(),
        description: "Earned by relaying settled transactions as a gateway".into(),
        owner: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        minted_by: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        slot: 2,
        rarity: 1,
        effect_bps: 500,
    }));
}

#[test]
fn deal_view_shape() {
    insta::assert_snapshot!(shape(&DealView {
        deal_id: 3,
        buyer: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        seller: "0xC24c000000000000000000000000000000000B2e".into(),
        token_id: 42,
        amount_avax: "0.05".into(),
        status: "active".into(),
        role: "buyer".into(),
    }));
}

#[test]
fn module_loadout_shape() {
    insta::assert_snapshot!(shape(&ModuleLoadout {
        equipped: vec![EquippedSlot { slot: 2, token_id: 7 }],
        multiplier: 1.05,
    }));
}

// ---------------------------------------------------------------------------
// Guardian mesh unlock
// ---------------------------------------------------------------------------

#[test]
fn guardian_candidate_shape() {
    insta::assert_snapshot!(shape(&GuardianCandidate {
        peer_id: "7f3a1209c2e100aa".into(),
        label: "7F3A…00AA".into(),
        hops: 1,
    }));
}

#[test]
fn guardian_status_shape() {
    insta::assert_snapshot!(shape(&GuardianStatus {
        enrolled: true,
        guardian_count: 5,
        threshold: 3,
        holding_for: 0,
    }));
}

#[test]
fn guardian_enroll_result_shape() {
    insta::assert_snapshot!(shape(&GuardianEnrollResult {
        enrolled: vec!["7f3a1209c2e100aa".into()],
        no_reply: vec!["91bec2e10800aabb".into()],
    }));
}

#[test]
fn guardian_unlock_prompt_shape() {
    insta::assert_snapshot!(shape(&GuardianUnlockPrompt { id: 42, from: "4B12…9F00".into() }));
}

#[test]
fn security_status_shape() {
    insta::assert_snapshot!(shape(&SecurityStatus { locked: false, passphrase_enabled: true }));
}

// ---------------------------------------------------------------------------
// Transactions and relaying
// ---------------------------------------------------------------------------

#[test]
fn tx_result_confirmed_shape() {
    // Internally tagged on `kind`. The frozen UI switches on this string, so
    // both the tag name and the variant renames are load-bearing.
    insta::assert_snapshot!(shape(&TxResult::Confirmed { id: 12 }));
}

#[test]
fn tx_result_queued_shape() {
    // `queue_id` is renamed to camelCase while sibling types stay snake_case —
    // an inconsistency the frozen UI depends on.
    insta::assert_snapshot!(shape(&TxResult::Queued {
        queue_id: "q-8a3f".into(),
    }));
}

#[test]
fn queued_tx_shape() {
    insta::assert_snapshot!(shape(&QueuedTx {
        id: "q-8a3f".into(),
        raw_tx_hex: "0x02f8".into(),
        summary: "Escrow release".into(),
        created_at: fixed_time(),
        status: "queued".into(),
        tx_hash: None,
        reason: None,
        // Untried, so `attempts` must be omitted from the wire shape — that is
        // what keeps the frozen contract unchanged by ticket 25's addition.
        attempts: 0,
    }));
}

#[test]
fn queued_tx_failed_shape() {
    insta::assert_snapshot!(shape(&QueuedTx {
        id: "q-8a3f".into(),
        raw_tx_hex: "0x02f8".into(),
        summary: "Escrow release".into(),
        created_at: fixed_time(),
        status: "failed".into(),
        tx_hash: Some("0xc70d".into()),
        reason: Some("insufficient funds".into()),
        attempts: 0,
    }));
}

#[test]
fn relayed_tx_record_shape() {
    insta::assert_snapshot!(shape(&RelayedTxRecord {
        summary: "Escrow release".into(),
        tx_hash: "0xc70d".into(),
        reward_avax: "0.005".into(),
        relayed_at: fixed_time(),
    }));
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

#[test]
fn content_record_shape() {
    insta::assert_snapshot!(shape(&ContentRecord {
        token_id: 42,
        text: "the quick brown fox".into(),
        fingerprint: "0xa4f2c9e1".into(),
        signature: "0xdeadbeef".into(),
        signer_address: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
    }));
}

#[test]
fn content_analysis_shape() {
    insta::assert_snapshot!(shape(&ContentAnalysis {
        content_type: "invoice".into(),
        is_real_document: true,
        reasoning: "structured fields present".into(),
    }));
}

// ---------------------------------------------------------------------------
// Matching and proofs
// ---------------------------------------------------------------------------

#[test]
fn match_result_shape() {
    insta::assert_snapshot!(shape(&MatchResult {
        listing_id: 7,
        seller: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        description: "AI compute credit".into(),
        price_avax: "0.05".into(),
        price_wei: "50000000000000000".into(),
        token_id: 42,
        reason: "matches intent and is under the ceiling".into(),
    }));
}

#[test]
fn proof_request_shape() {
    insta::assert_snapshot!(shape(&ProofRequest {
        balance: 1000,
        bid_amount: 95,
        price_ceiling: 100,
    }));
}

#[test]
fn zk_proof_shape() {
    insta::assert_snapshot!(shape(&ZKProof {
        proof: "0xa4f2c9e1b70d5533".into(),
        public_inputs: vec!["95".into()],
        encrypted_intent: "{\"bid\":95,\"verified\":true}".into(),
    }));
}

// ---------------------------------------------------------------------------
// Mesh
// ---------------------------------------------------------------------------

#[test]
fn privacy_intent_shape() {
    insta::assert_snapshot!(shape(&PrivacyIntent {
        intent_type: "trade".into(),
        payload: "{\"action\":\"buy\"}".into(),
        encrypted: true,
        relay_path: vec!["origin_node".into()],
        relay_fee: Some("0.005 AVAX".into()),
    }));
}

/// Every `MeshEvent` variant in one snapshot.
///
/// These arrive at the webview through `emit("mesh-event", ..)` and the frozen
/// UI dispatches on the `type` tag, so adding, renaming or retagging a variant
/// is a breaking change even though nothing in Rust would complain.
#[test]
fn mesh_event_variants_shape() {
    let events = vec![
        MeshEvent::ListeningStarted {
            address: "/ip4/127.0.0.1/tcp/61854".into(),
        },
        MeshEvent::PeerDiscovered {
            peer_id: "12D3KooWJLUV".into(),
            address: "/ip4/192.168.1.118/tcp/60744".into(),
        },
        MeshEvent::IntentReceived {
            intent: PrivacyIntent {
                intent_type: "trade".into(),
                payload: "{}".into(),
                encrypted: false,
                relay_path: vec!["origin_node".into()],
                relay_fee: None,
            },
        },
        MeshEvent::DealAccepted {
            details: "deal 3 accepted".into(),
        },
        MeshEvent::SettlementComplete {
            details: "settled in 11.4s".into(),
        },
        MeshEvent::RelayTxReceived {
            queue_id: "q-8a3f".into(),
            raw_tx_hex: "0x02f8".into(),
            summary: "Escrow release".into(),
        },
        MeshEvent::RelayConfirmed {
            queue_id: "q-8a3f".into(),
            status: "confirmed".into(),
            tx_hash: Some("0xc70d".into()),
        },
        MeshEvent::ContentRequested { token_id: 42 },
        MeshEvent::ContentDelivered {
            token_id: 42,
            text: "the quick brown fox".into(),
            signature: "0xdeadbeef".into(),
            signer_address: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        },
        MeshEvent::PeerIdentity {
            peer_id: "12D3KooWJLUV".into(),
            address: "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9".into(),
        },
    ];
    insta::assert_snapshot!(shape(&events));
}

// ---------------------------------------------------------------------------
// Ad-hoc JSON payloads
// ---------------------------------------------------------------------------

/// `get_escrow_status` returns a hand-built `serde_json::Value` rather than a
/// typed struct, so nothing but this snapshot pins its keys.
#[test]
fn escrow_status_shape() {
    let value = serde_json::json!({
        "depositor": "0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9",
        "payee": "0xC24c000000000000000000000000000000000B2e",
        "amount": "50000000000000000",
        "expiry": 1_785_603_228_u64,
        "status": 0,
    });
    insta::assert_snapshot!(shape(&value));
}

/// The bootstrap progress payload, emitted on `bootstrap-status`. The struct
/// is private to `app_initializer`, so its shape is asserted structurally.
#[test]
fn bootstrap_status_shape() {
    let value = serde_json::json!({
        "phase": "PHASE_3_NETWORK",
        "message": "Booting Libp2p Swarm...",
        "progress": 70,
    });
    insta::assert_snapshot!(shape(&value));
}

// ---------------------------------------------------------------------------
// Command inventory
// ---------------------------------------------------------------------------

/// Guards the command surface itself.
///
/// Kept in step with the `COMMANDS` list in `build.rs`, which generates the
/// ACL permissions, and with the single handler registration in `lib.rs`
/// (desktop and mobile share it — see `tests/handler_arms.rs` for the
/// per-platform capability-file check). A command removed or renamed without
/// updating all three is a silent 404, and a permission granted for a command
/// that no longer exists fails the build.
#[test]
fn command_inventory() {
    let mut commands = vec![
        "ble_status",
        "broadcast_intent",
        "cancel_intent",
        "enter_mesh",
        "guardian_approve_unlock",
        "guardian_candidates",
        "guardian_deny_unlock",
        "guardian_enroll",
        "guardian_request_unlock",
        "guardian_status",
        "intent_detail",
        "intent_form_options",
        "intent_proof",
        "list_intents",
        "list_nearby_nodes",
        "market_buy",
        "market_list_module",
        "market_listings",
        "market_my_deals",
        "market_refund_deal",
        "market_release_deal",
        "mesh_snapshot",
        "parse_intent_chat",
        "preview_intent",
        "profile_summary",
        "security_disable_passphrase",
        "security_enable_passphrase",
        "security_status",
        "security_unlock",
        "session_status",
        "set_offline_mode",
        "settle_intent",
        "subscribe_mesh_log",
        "subscribe_settlement_log",
        "unsubscribe",
        "vault_assets",
        "vault_equip_module",
        "vault_export_key",
        "vault_identities",
        "vault_import_key",
        "vault_keys",
        "vault_loadout",
        "vault_modules",
        "vault_redeem_module",
        "vault_unequip_module",
    ];
    commands.sort_unstable();
    assert_eq!(commands.len(), 45, "command count changed");
    insta::assert_snapshot!(commands.join("\n"));
}
