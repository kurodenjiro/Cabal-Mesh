//! Both invoke-handler arms carry the same reshaped commands.
//!
//! # The failure this exists for
//!
//! `run()` builds two `generate_handler!` lists: one for desktop with the
//! frozen legacy surface, one for mobile without it. A command added to the
//! desktop arm and forgotten on the mobile one **compiles, passes every test,
//! and is granted by `capabilities/mobile.json`** — the ACL allows it, the
//! command simply is not registered, so the invoke fails at runtime on the
//! phone and nowhere else.
//!
//! That is exactly what happened to `ble_status`: it was granted, present in
//! the generated ACL schema, and missing from the mobile arm. The symptom was
//! a panel that silently rendered nothing on device while every test was
//! green, and it cost a full simulator build to find.
//!
//! # Why this reads the source
//!
//! A macro's contents are not introspectable at runtime, and building a mock
//! app per platform arm proves only that the arm the test was compiled for
//! exists. Reading `lib.rs` is crude, and it is the only thing that actually
//! compares the two lists.

use std::collections::BTreeSet;

/// The reshaped command surface — the commands both platforms must have.
///
/// The legacy desktop commands are deliberately absent: they exist only on
/// desktop, which is the whole point of the split.
const SHARED: &[&str] = &[
    "unsubscribe",
    "session_status",
    "enter_mesh",
    "mesh_snapshot",
    "subscribe_mesh_log",
    "list_nearby_nodes",
    "ble_status",
    "list_intents",
    "intent_form_options",
    "preview_intent",
    "broadcast_intent",
    "intent_detail",
    "subscribe_settlement_log",
    "settle_intent",
    "cancel_intent",
    "intent_proof",
    "vault_assets",
    "vault_identities",
    "vault_keys",
    "profile_summary",
    "set_offline_mode",
];

fn source() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("lib.rs is readable from the crate root")
}

/// The command names inside each `generate_handler!` invocation.
fn handler_arms(source: &str) -> Vec<BTreeSet<String>> {
    let mut arms = Vec::new();
    let mut rest = source;

    while let Some(start) = rest.find("tauri::generate_handler![") {
        let body_start = start + "tauri::generate_handler![".len();
        let Some(end) = rest[body_start..].find(']') else {
            break;
        };
        let body = &rest[body_start..body_start + end];

        arms.push(
            body.split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .filter_map(|entry| entry.rsplit("::").next())
                .map(str::to_string)
                .collect(),
        );

        rest = &rest[body_start + end..];
    }

    arms
}

#[test]
fn there_are_exactly_two_handler_arms() {
    // If a third appears, this file's assumption that "both arms" means
    // "desktop and mobile" needs revisiting rather than silently passing.
    let arms = handler_arms(&source());
    assert_eq!(
        arms.len(),
        2,
        "expected a desktop arm and a mobile arm, found {}",
        arms.len()
    );
}

#[test]
fn every_shared_command_is_registered_on_both_platforms() {
    let arms = handler_arms(&source());

    for (index, arm) in arms.iter().enumerate() {
        let missing: Vec<&str> = SHARED
            .iter()
            .copied()
            .filter(|command| !arm.contains(*command))
            .collect();

        assert!(
            missing.is_empty(),
            "handler arm {index} is missing {missing:?} — granted by the ACL, \
             absent from the handler, and therefore broken only on that platform"
        );
    }
}

#[test]
fn the_shared_list_matches_the_acl_manifest() {
    // `build.rs` generates a permission per command in its own COMMANDS list.
    // A command in a handler but not in that list has no permission to grant,
    // and one in the list but no handler is a permission for nothing.
    let build = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"))
        .expect("build.rs is readable");

    for command in SHARED {
        assert!(
            build.contains(&format!("\"{command}\"")),
            "`{command}` is registered as a command but has no entry in build.rs COMMANDS, \
             so no permission is generated for it and the ACL denies every call"
        );
    }
}

#[test]
fn the_mobile_capability_grants_every_shared_command() {
    // The third place the same name has to appear. Granting is separate from
    // registering, and missing either one fails only on a device.
    let capability =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/mobile.json"))
            .expect("mobile.json is readable");

    for command in SHARED {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(
            capability.contains(&permission),
            "`{permission}` is not granted in capabilities/mobile.json, so `{command}` \
             is denied on the phone however correctly it is registered"
        );
    }
}
