import React, { useState, useEffect } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { formatEther } from "ethers";
import { PixelClassIcon } from "./icons/PixelClassIcon";

interface WalletCabinetProps {
    visible: boolean;
    onClose: () => void;
    peerCount?: number;
}

interface CompressedAsset {
    id: string;
    amount: string; // decimal wei string
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

export const WalletCabinet: React.FC<WalletCabinetProps> = ({ visible, onClose, peerCount = 0 }) => {
    const [bridgeStatus, setBridgeStatus] = useState<string | null>(null);
    const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
    const [identity, setIdentity] = useState<IdentityView | null>(null);
    const [syncing, setSyncing] = useState(false);
    const [copied, setCopied] = useState(false);
    const [showImport, setShowImport] = useState(false);
    const [importKey, setImportKey] = useState("");
    const [importing, setImporting] = useState(false);
    // Tauri's webview doesn't reliably surface window.confirm() as a real,
    // clickable dialog — it can silently no-op, making these destructive
    // buttons look unresponsive. Use an in-app two-step confirm instead.
    const [confirming, setConfirming] = useState<"logout" | "delete" | null>(null);
    const [actionMessage, setActionMessage] = useState<string | null>(null);
    const [privateKey, setPrivateKey] = useState<string | null>(null);
    const [revealingKey, setRevealingKey] = useState(false);
    const [keyCopied, setKeyCopied] = useState(false);
    useEffect(() => {
        if (!actionMessage) return;
        const timer = setTimeout(() => setActionMessage(null), 6000);
        return () => clearTimeout(timer);
    }, [actionMessage]);

    const handleCopyAddress = async () => {
        if (!identity) return;
        try {
            await navigator.clipboard.writeText(identity.address);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
        } catch (e) {
            console.error("Failed to copy address:", e);
        }
    };

    const fetchSnapshot = async () => {
        try {
            const data = await invoke<Snapshot>("get_wallet_snapshot");
            if (data && data.assets) {
                setSnapshot(data);
            }
        } catch (e) {
            console.error("Failed to load snapshot", e);
        }
    };

    // get_wallet_snapshot only reads the last-saved local snapshot — it does not
    // hit the chain. A real balance refresh needs to re-sync from RPC first.
    const refreshBalanceFromChain = async () => {
        try {
            await invoke("sync_blockchain_state", { wallet: "" });
        } catch (e) {
            console.error("Failed to sync balance from chain:", e);
        }
        fetchSnapshot();
    };

    useEffect(() => {
        if (!visible) return;
        invoke("get_bridge_status").then((status: any) => setBridgeStatus(status)).catch(console.error);
        fetchIdentity();
        refreshBalanceFromChain();

        // Auto-refresh the balance while the cabinet is open, instead of only on manual click.
        const interval = setInterval(refreshBalanceFromChain, 15000);
        return () => clearInterval(interval);
    }, [visible]);

    const fetchIdentity = async () => {
        try {
            const ids = await invoke<IdentityView[]>("get_identity");
            setIdentity(ids?.[0] || null);
        } catch (e) {
            console.error("Failed to fetch identity", e);
        }
    };

    const handleSync = async () => {
        setSyncing(true);
        try {
            console.log("🔄 Syncing Blockchain state...");
            // Backend ignores the argument now, uses internal identity
            await invoke("sync_blockchain_state", { wallet: "" });

            // Auto-enable Instant Session once synced
            const session = await invoke("enable_instant_session");
            console.log("⚡ Instant Session:", session);

            // Update UI
            const status = await invoke("get_bridge_status");
            setBridgeStatus(status as string);

            // Allow time for FS write then fetch
            setTimeout(fetchSnapshot, 1000);
        } catch (e) {
            console.error("Sync failed:", e);
        } finally {
            setSyncing(false);
        }
    };

    const handleDelete = async () => {
        setConfirming(null);
        try {
            await invoke("delete_wallet_snapshot");
            setSnapshot(null);
            setBridgeStatus(null);
            setActionMessage("Local data deleted.");
        } catch (e) {
            console.error("Delete failed:", e);
            setActionMessage("Failed to delete local data: " + e);
        }
    };

    const handleLogout = async () => {
        setConfirming(null);
        try {
            await invoke("logout_wallet");
            setActionMessage("Logged out — reloading with your new wallet...");
            // Every other screen (balance chip, mesh character label, Trading
            // Post, etc.) independently caches the identity it fetched at its
            // own mount time — a full reload is the only way to guarantee all
            // of them pick up the new one instead of showing the old address.
            setTimeout(() => window.location.reload(), 800);
        } catch (e) {
            console.error("Logout failed:", e);
            setActionMessage("Failed to log out: " + e);
        }
    };

    const handleRevealKey = async () => {
        setRevealingKey((v) => !v);
        if (privateKey) return;
        try {
            const key = await invoke<string | null>("get_primary_private_key");
            setPrivateKey(key);
        } catch (e) {
            console.error("Failed to fetch private key:", e);
            setActionMessage("Failed to fetch private key: " + e);
        }
    };

    const handleCopyKey = async () => {
        if (!privateKey) return;
        try {
            await navigator.clipboard.writeText(privateKey);
            setKeyCopied(true);
            setTimeout(() => setKeyCopied(false), 1500);
        } catch (e) {
            console.error("Failed to copy private key:", e);
        }
    };

    const handleImport = async () => {
        const key = importKey.trim();
        if (!key) return;
        setImporting(true);
        try {
            await invoke("import_wallet", { privateKeyHex: key, alias: "Imported Fox", emoji: "🦊" });
            setImportKey("");
            setShowImport(false);
            setActionMessage("Wallet imported — reloading...");
            // Same reasoning as logout: every screen needs a fresh fetch, not
            // just this modal's own local state.
            setTimeout(() => window.location.reload(), 800);
        } catch (e) {
            console.error("Import failed:", e);
            setActionMessage("Failed to import wallet — check that the private key is valid: " + e);
        } finally {
            setImporting(false);
        }
    };

    if (!visible) return null;

    const nativeBalance = snapshot?.assets?.find((a) => a.symbol === "AVAX");
    const balanceText = nativeBalance ? parseFloat(formatEther(nativeBalance.amount)).toFixed(5) : "0.00000";

    return (
        <motion.div
            className="absolute inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm text-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
        >
            <div className="w-[600px] max-h-[85vh] flex flex-col hud-frame border border-nobody-primary/20 bg-nobody-charcoal shadow-card-lg overflow-hidden text-nobody-primary">

                {/* Header */}
                <div className="bg-slate-50 px-5 py-3 border-b border-nobody-primary/20 flex justify-between items-center text-xs">
                    <span className="text-nobody-primary font-pixel text-[10px] tracking-wide">WALLET</span>
                    <div className="flex items-center gap-3">
                        <span className="text-nobody-primary font-medium">
                            {bridgeStatus?.includes("Active") ? "🟢 Agent Access: On" : "⚪ Agent Access: Off"}
                        </span>
                        <button onClick={onClose} className="text-slate-400 hover:text-slate-700 transition-colors">✕</button>
                    </div>
                </div>

                <div className="p-6 space-y-6 overflow-y-auto">

                    {/* Balance — the single most important number, given its own space */}
                    <div className="pixel-corners-sm bg-nobody-primary-soft/30 border border-nobody-primary/20 p-5 text-center">
                        <div className="text-xs text-slate-500 mb-1">🎒 Your Gold</div>
                        <div className="text-3xl font-bold text-nobody-primary">{balanceText} AVAX</div>
                        {identity && (
                            <div className="flex items-center justify-center gap-1.5 text-[11px] text-slate-400 font-mono mt-2">
                                <PixelClassIcon address={identity.address} size={14} />
                                {identity.emoji || "👻"} {identity.address.slice(0, 10)}...{identity.address.slice(-8)}
                                <button
                                    onClick={handleCopyAddress}
                                    title="Copy full address"
                                    className="text-slate-400 hover:text-nobody-primary transition-colors"
                                >
                                    {copied ? "✓" : "📋"}
                                </button>
                            </div>
                        )}
                        <div className="flex justify-center gap-4 mt-3 text-xs">
                            <button onClick={handleSync} disabled={syncing} className="text-nobody-primary hover:underline font-semibold disabled:opacity-50">
                                {syncing ? "Refreshing..." : "🔄 Refresh Balance"}
                            </button>
                            <button onClick={handleRevealKey} className="text-slate-400 hover:text-nobody-primary font-semibold">
                                {revealingKey ? "🔑 Hide private key" : "🔑 Show private key"}
                            </button>
                        </div>
                        {revealingKey && (
                            <div className="mt-3 pixel-corners-sm bg-red-50 border border-red-200 p-3 text-left space-y-1.5">
                                <div className="text-[10px] text-red-700">
                                    ⚠️ Anyone with this key can spend this wallet's funds. Save it somewhere safe — it's the only way to get back to this wallet after logging out or importing another one.
                                </div>
                                {privateKey ? (
                                    <div className="flex items-center gap-2">
                                        <code className="flex-1 text-[10px] font-mono text-slate-700 bg-white border border-slate-200 pixel-corners-sm px-2 py-1.5 break-all">
                                            {privateKey}
                                        </code>
                                        <button onClick={handleCopyKey} title="Copy private key" className="text-slate-400 hover:text-nobody-primary transition-colors shrink-0">
                                            {keyCopied ? "✓" : "📋"}
                                        </button>
                                    </div>
                                ) : (
                                    <div className="text-[10px] text-slate-400 italic">Loading...</div>
                                )}
                            </div>
                        )}
                    </div>

                    {/* Danger zone — visually separated and de-emphasized so it's never confused with a normal action */}
                    <div className="pt-2 border-t border-slate-100 space-y-2">
                        {actionMessage && (
                            <div className="text-[11px] text-nobody-primary text-right">{actionMessage}</div>
                        )}
                        {confirming === "logout" && (
                            <div className="pixel-corners-sm bg-red-50 border border-red-200 p-3 space-y-2">
                                <div className="text-[11px] text-red-700">
                                    Log out of this wallet and generate a brand-new one? Make sure you've saved this wallet's private key if you want to come back to it later.
                                </div>
                                <div className="flex justify-end gap-3 text-[11px]">
                                    <button onClick={() => setConfirming(null)} className="text-slate-400 hover:text-slate-600">Cancel</button>
                                    <button onClick={handleLogout} className="text-red-600 font-semibold hover:underline">Yes, log out</button>
                                </div>
                            </div>
                        )}
                        {confirming === "delete" && (
                            <div className="pixel-corners-sm bg-red-50 border border-red-200 p-3 space-y-2">
                                <div className="text-[11px] text-red-700">
                                    ⚠️ This deletes the local encrypted balance snapshot. You'll need to sync again. Continue?
                                </div>
                                <div className="flex justify-end gap-3 text-[11px]">
                                    <button onClick={() => setConfirming(null)} className="text-slate-400 hover:text-slate-600">Cancel</button>
                                    <button onClick={handleDelete} className="text-red-600 font-semibold hover:underline">Yes, delete</button>
                                </div>
                            </div>
                        )}
                        {!showImport && !confirming ? (
                            <div className="flex justify-end gap-4 flex-wrap">
                                <button
                                    onClick={() => setShowImport(true)}
                                    className="text-[11px] text-slate-400 hover:text-nobody-primary transition-colors"
                                >
                                    🔑 Import a different wallet
                                </button>
                                <button
                                    onClick={() => setConfirming("logout")}
                                    className="text-[11px] text-slate-400 hover:text-red-600 transition-colors"
                                >
                                    Log out this wallet
                                </button>
                                <button
                                    onClick={() => setConfirming("delete")}
                                    className="text-[11px] text-slate-400 hover:text-red-600 transition-colors"
                                >
                                    Erase all local data
                                </button>
                            </div>
                        ) : !showImport ? null : (
                            <div className="pixel-corners-sm bg-black/20 border border-nobody-primary/20 p-3 space-y-2">
                                <div className="text-[11px] text-slate-400">
                                    Paste a private key (0x...) to switch to that wallet. Your current wallet's key will be gone unless you saved it.
                                </div>
                                <input
                                    type="password"
                                    value={importKey}
                                    onChange={(e) => setImportKey(e.target.value)}
                                    placeholder="0x..."
                                    className="w-full bg-nobody-charcoal border border-slate-600 pixel-corners-sm px-2 py-1.5 text-xs text-slate-200 font-mono"
                                />
                                <div className="flex justify-end gap-3 text-[11px]">
                                    <button
                                        onClick={() => { setShowImport(false); setImportKey(""); }}
                                        className="text-slate-400 hover:text-slate-200"
                                    >
                                        Cancel
                                    </button>
                                    <button
                                        onClick={handleImport}
                                        disabled={importing || !importKey.trim()}
                                        className="text-nobody-primary hover:underline font-semibold disabled:opacity-50"
                                    >
                                        {importing ? "Importing..." : "Import & Switch"}
                                    </button>
                                </div>
                            </div>
                        )}
                    </div>

                </div>

                {/* Footer */}
                <div className="bg-slate-50 border-t border-slate-200 px-5 py-2 flex justify-between items-center text-xs">
                    <span className="text-slate-400">Mesh: {peerCount} peers connected</span>
                    <button
                        onClick={onClose}
                        className="bg-nobody-charcoal text-slate-500 font-pixel text-[10px] px-6 py-2 hover:text-nobody-primary transition-colors pixel-corners-sm border border-slate-300 hover:border-nobody-primary shadow-card"
                    >
                        [ BACK ]
                    </button>
                </div>
            </div>
        </motion.div>
    );

};
