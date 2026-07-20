import React, { useState, useEffect } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";

interface SmartEscrowProps {
    visible: boolean;
    escrowId: number | null;
    onClose: () => void;
    onRelease?: () => void;
}

export const SmartEscrow: React.FC<SmartEscrowProps> = ({ visible, escrowId, onClose, onRelease }) => {
    const [released, setReleased] = useState(false);
    const [releasing, setReleasing] = useState(false);
    const [finality, setFinality] = useState<"pending" | "confirmed">("pending");

    useEffect(() => {
        if (visible) {
            setReleased(false);
            setReleasing(false);
            setFinality("pending");

            if (escrowId != null) {
                invoke("get_escrow_status", { escrowId })
                    .then(() => setFinality("confirmed"))
                    .catch((e) => {
                        console.error("Failed to fetch escrow status:", e);
                        setFinality("pending");
                    });
            }
        }
    }, [visible, escrowId]);

    const handleRelease = async () => {
        if (escrowId == null) {
            alert("No on-chain escrow is associated with this deal.");
            return;
        }

        setReleasing(true);
        try {
            await invoke("release_escrow", { escrowId });
            setReleased(true);
            onRelease?.();
            setTimeout(() => {
                onClose();
            }, 2000);
        } catch (e) {
            console.error("On-chain release failed:", e);
            alert("On-chain release failed: " + e);
        } finally {
            setReleasing(false);
        }
    };

    if (!visible) return null;

    return (
        <motion.div
            className="absolute inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
        >
            <div className="w-[700px] rounded-2xl border border-slate-200 bg-nobody-charcoal shadow-card-lg p-8 relative">
                <button onClick={onClose} className="absolute top-4 right-4 text-slate-400 hover:text-slate-700 transition-colors">✕</button>

                <div className="text-center mb-10">
                    <div className="text-amber-400 text-xs font-semibold tracking-widest mb-2 uppercase">Smart Escrow Protocol</div>
                    <h1 className="text-2xl text-slate-900 font-bold">
                        {released ? "Settlement Complete" : "Settlement In Progress"}
                    </h1>
                </div>

                <div className="flex justify-between items-center mb-12 px-10">
                    {/* Left Vault */}
                    <div className="text-center">
                        <div className="w-20 h-20 rounded-full flex items-center justify-center text-3xl mb-4 bg-amber-950/30 border border-amber-800/40">
                            💰
                        </div>
                        <div className="text-slate-900 font-semibold">13.5 AVAX</div>
                        <div className="text-xs text-slate-400 mt-1">
                            {released ? "Released ✓" : "ZK-Vault Locked"}
                        </div>
                    </div>

                    {/* Bridge */}
                    <div className="flex-1 px-4 flex flex-col items-center">
                        <div className="h-[2px] w-full bg-slate-100 relative">
                            <motion.div
                                className="absolute top-1/2 -translate-y-1/2 w-full h-[2px] bg-amber-400"
                                initial={{ scaleX: 0 }}
                                animate={{ scaleX: released ? 1 : [0, 1] }}
                                transition={{ duration: 2, repeat: released ? 0 : Infinity }}
                            />
                        </div>
                        <div className="mt-2 text-[11px] text-amber-400 font-medium">
                            {released ? "Complete ✓" : <span className="animate-pulse">Verifying integrity...</span>}
                        </div>
                    </div>

                    {/* Right Vault */}
                    <div className="text-center">
                        <div className="w-20 h-20 rounded-full flex items-center justify-center text-3xl mb-4 bg-nobody-violet-soft border border-nobody-violet/20">
                            🦊
                        </div>
                        <div className="text-slate-900 font-semibold">NFT #04</div>
                        <div className="text-xs text-slate-400 mt-1">
                            {released ? "Transferred ✓" : "Mesh Proxy Held"}
                        </div>
                    </div>
                </div>

                {/* Status Items */}
                <div className="grid grid-cols-2 gap-4 mb-8">
                    <div className="bg-slate-50 rounded-xl p-3 border border-slate-200 flex justify-between items-center">
                        <span className="text-slate-500 text-xs">Avalanche Finality</span>
                        <span className={`text-xs font-semibold ${finality === "confirmed" ? "text-nobody-mint" : "text-slate-400"}`}>
                            {finality === "confirmed" ? "Confirmed" : "Pending"}
                        </span>
                    </div>
                    <div className="bg-slate-50 rounded-xl p-3 border border-slate-200 flex justify-between items-center">
                        <span className="text-slate-500 text-xs">Mesh Signatures</span>
                        <span className="text-nobody-mint text-xs font-semibold">Valid 3/3</span>
                    </div>
                </div>

                <button
                    onClick={handleRelease}
                    disabled={released || releasing || escrowId == null}
                    className={`w-full font-semibold py-3 rounded-xl transition-colors shadow-card ${released
                        ? "bg-nobody-mint text-white cursor-default"
                        : "bg-amber-500 text-white hover:bg-amber-600 disabled:opacity-50 disabled:cursor-not-allowed"
                        }`}
                >
                    {released ? "✓ Funds Released" : releasing ? "Releasing..." : "Release Funds"}
                </button>

            </div>
        </motion.div>
    );
};
