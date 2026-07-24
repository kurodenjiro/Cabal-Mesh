import React, { useState, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { formatEther } from "ethers";
import pouchIcon from "../assets/icons/icon_pouch.png";

interface CompressedAsset {
    id: string;
    amount: string;
    symbol: string;
    owner: string;
}

interface Snapshot {
    timestamp: string;
    assets: CompressedAsset[];
    signature: string;
}

interface IdentityView {
    alias: string;
    emoji: string;
    address: string;
}

interface WalletBalanceCompactProps {
    /** Opens the full Wallet screen for less-common actions (delegate agent, erase local data). */
    onOpenWallet?: () => void;
}

/** Always-visible AVAX balance + address, right on the Nexus screen — no need
 * to open a separate Wallet page just to see or copy your address. */
export const WalletBalanceCompact: React.FC<WalletBalanceCompactProps> = ({ onOpenWallet }) => {
    const [showInfo, setShowInfo] = useState(false);
    const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
    const [identity, setIdentity] = useState<IdentityView | null>(null);
    const [copied, setCopied] = useState(false);

    // get_wallet_snapshot only reads the last-saved local snapshot — it does not
    // hit the chain. A real balance needs to re-sync from RPC first.
    const refresh = useCallback(async () => {
        try {
            await invoke("sync_blockchain_state", { wallet: "" });
        } catch (e) {
            console.error("Failed to sync balance from chain:", e);
        }
        invoke<Snapshot>("get_wallet_snapshot")
            .then((data) => data?.assets && setSnapshot(data))
            .catch((e) => console.error("Failed to load wallet snapshot:", e));
    }, []);

    useEffect(() => {
        // Backend identity-loading is async and can still be in progress at mount —
        // a one-shot fetch can race it and come back empty forever, so poll until ready.
        let cancelled = false;
        const tryFetchIdentity = () => {
            invoke<IdentityView[]>("get_identity")
                .then((ids) => {
                    if (cancelled) return;
                    if (ids?.[0]) setIdentity(ids[0]);
                    else setTimeout(tryFetchIdentity, 1000);
                })
                .catch((e) => {
                    console.error("Failed to fetch identity, retrying:", e);
                    if (!cancelled) setTimeout(tryFetchIdentity, 1000);
                });
        };
        tryFetchIdentity();
        refresh();
        const interval = setInterval(refresh, 15000);
        return () => { cancelled = true; clearInterval(interval); };
    }, [refresh]);

    const handleCopy = async (e: React.MouseEvent) => {
        e.stopPropagation();
        if (!identity) return;
        try {
            await navigator.clipboard.writeText(identity.address);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
        } catch (err) {
            console.error("Failed to copy address:", err);
        }
    };

    const nativeBalance = snapshot?.assets?.find((a) => a.symbol === "AVAX");
    const balanceText = nativeBalance ? parseFloat(formatEther(nativeBalance.amount)).toFixed(5) : "0.00000";

    return (
        <div className="relative">
            <button
                onClick={() => setShowInfo(!showInfo)}
                className="h-8 flex items-center gap-2 bg-nobody-charcoal pixel-corners-sm px-3 border border-nobody-primary/20 shadow-card"
            >
                <span className="text-[10px] text-slate-500 font-medium flex items-center gap-1">
                    <img src={pouchIcon} alt="" draggable={false} style={{ width: 12, height: 15, imageRendering: "pixelated" }} />
                    {balanceText} AVAX
                </span>
                {identity && (
                    <>
                        <span className="text-[10px] text-slate-400 font-mono">{identity.address.slice(0, 6)}...{identity.address.slice(-4)}</span>
                        <span onClick={handleCopy} title="Copy full address" className="text-slate-400 hover:text-nobody-primary transition-colors">
                            {copied ? "✓" : "📋"}
                        </span>
                    </>
                )}
            </button>

            <AnimatePresence>
                {showInfo && (
                    <motion.div
                        initial={{ opacity: 0, y: -4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -4 }}
                        className="absolute top-10 right-0 w-56 pixel-corners-sm bg-nobody-charcoal border border-nobody-primary/30 shadow-card-lg p-3 z-50 text-xs space-y-2"
                    >
                        <div className="text-slate-500">
                            Real on-chain balance for your primary identity on Avalanche Fuji.
                        </div>
                        {onOpenWallet && (
                            <button onClick={onOpenWallet} className="text-nobody-primary hover:underline font-semibold">
                                Manage wallet (log out / import) →
                            </button>
                        )}
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
};

export default WalletBalanceCompact;
