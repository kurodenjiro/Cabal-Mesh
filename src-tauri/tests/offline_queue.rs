//! Offline queue durability.
//!
//! The profile screen's offline switch promises *"Intents queue locally.
//! Nothing leaves this device."* Keeping the first half of that promise means
//! the queue survives the thing mobile does constantly: getting killed without
//! warning.
//!
//! These tests exercise the persisted shape rather than a live chain, because
//! submission needs an RPC endpoint and the durability guarantee does not.

use cabalmesh_lib::blockchain_bridge::QueuedTx;
use chrono::Utc;

fn queued(id: &str) -> QueuedTx {
    QueuedTx {
        id: id.into(),
        raw_tx_hex: "0x02f8".into(),
        summary: "Escrow release".into(),
        created_at: Utc::now(),
        status: "queued".into(),
        tx_hash: None,
        reason: None,
        attempts: 0,
    }
}

/// A queue written before a kill is readable after it.
///
/// The atomic write from ticket 17 is what makes this true; before it, a kill
/// mid-write could leave a truncated file and lose every queued transaction.
#[test]
fn the_queue_survives_being_written_and_reopened() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = cabal_store::JsonStore::new(dir.path().join("pending_relay_txs.json"));

    let original = vec![queued("q-1"), queued("q-2")];
    store.save(&original).unwrap();

    // A fresh store at the same path is what a relaunched process sees.
    let reopened = cabal_store::JsonStore::new(dir.path().join("pending_relay_txs.json"));
    let recovered: Vec<QueuedTx> = reopened.load().unwrap();

    assert_eq!(recovered.len(), 2);
    assert_eq!(recovered[0].id, "q-1");
    assert_eq!(recovered[0].raw_tx_hex, "0x02f8");
}

/// A queue file written before `attempts` existed still loads.
///
/// Real installations have one. Failing to read it would drop transactions the
/// user is waiting on, which is exactly what the queue exists to prevent.
#[test]
fn a_queue_file_without_the_attempts_field_still_loads() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("pending_relay_txs.json");
    std::fs::write(
        &path,
        r#"[{"id":"q-old","raw_tx_hex":"0x02f8","summary":"Escrow release",
             "created_at":"2026-05-18T14:32:00Z","status":"queued","tx_hash":null,"reason":null}]"#,
    )
    .unwrap();

    let recovered: Vec<QueuedTx> = cabal_store::JsonStore::new(&path).load().unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].attempts, 0);
}

/// An untried entry serializes without `attempts`.
///
/// This is what keeps the frozen desktop contract unchanged: the field was
/// added for retry bookkeeping, and the UI that predates it must see exactly
/// the shape it always saw.
#[test]
fn an_untried_entry_omits_attempts_from_the_wire_shape() {
    let json = serde_json::to_string(&queued("q-1")).unwrap();
    assert!(
        !json.contains("attempts"),
        "adding a field changed the frozen shape: {json}"
    );
}

/// A retried entry does carry the count, so it survives a restart.
///
/// Without persistence, every relaunch would reset the counter and an unminable
/// transaction would be retried forever — draining the battery on a chain that
/// will never accept it.
#[test]
fn a_retried_entry_persists_its_attempt_count() {
    let mut entry = queued("q-1");
    entry.attempts = 3;

    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("attempts"));

    let recovered: QueuedTx = serde_json::from_str(&json).unwrap();
    assert_eq!(recovered.attempts, 3);
}

/// The retry ceiling is low enough to stop, high enough to ride out a flaky
/// connection.
#[test]
fn the_retry_ceiling_is_bounded() {
    use cabalmesh_lib::blockchain_bridge::BlockchainBridge;
    assert!(BlockchainBridge::MAX_ATTEMPTS >= 3, "too few retries to survive a flap");
    assert!(BlockchainBridge::MAX_ATTEMPTS <= 10, "retrying this often is a battery cost");
}
