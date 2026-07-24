import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// Not an actual paid-out reward (no counterparty settlement system exists yet) —
// a deterministic estimate derived from real relayed bytes, for display only.
export const RATE_PER_BYTE_AVAX = 0.000001;

function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(2)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

/** Polls the real mesh relay byte counter (src-tauri/src/mesh.rs) while Relay Mode is on. */
export function useRelayStats(isRelaying: boolean) {
    const [bytesProcessed, setBytesProcessed] = useState(0);
    // Persisted multiplier (starts at 1.0) — permanently raised by redeeming an
    // "AI Compute Credit" / "Relay Bandwidth Credit" item (see RedeemVoucher.tsx).
    // Still just scales the same local estimate, not a real payout.
    const [boostMultiplier, setBoostMultiplier] = useState(1);

    useEffect(() => {
        invoke<number>("get_relay_boost")
            .then(setBoostMultiplier)
            .catch((e) => console.error("Failed to fetch relay boost:", e));
    }, []);

    useEffect(() => {
        if (!isRelaying) return;

        const poll = () => {
            invoke<number>("get_relay_stats")
                .then(setBytesProcessed)
                .catch((e) => console.error("Failed to fetch relay stats:", e));
        };

        poll();
        const interval = setInterval(poll, 2000);
        return () => clearInterval(interval);
    }, [isRelaying]);

    return {
        traffic: formatBytes(bytesProcessed),
        earnings: (bytesProcessed * RATE_PER_BYTE_AVAX * boostMultiplier).toFixed(5) + " AVAX",
        bytesProcessed,
        boostMultiplier,
    };
}
