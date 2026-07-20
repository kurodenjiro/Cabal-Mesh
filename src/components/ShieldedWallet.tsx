import React, { useState } from "react";
import { motion } from "framer-motion";

interface ShieldedWalletProps {
    visible: boolean;
    onClose: () => void;
}

export const ShieldedWallet: React.FC<ShieldedWalletProps> = ({ visible, onClose }) => {
    const [revealBalance, setRevealBalance] = useState(false);

    // Mock Data
    const balance = "142.5 AVAX";
    const pendingIntents = [
        { id: "#042b", amount: "9.1 AVAX", reason: "Reserved for ESP32 Trade" },
        { id: "#042c", amount: "1.2 AVAX", reason: "Reserved for Mesh Gas" },
    ];

    if (!visible) return null;

    return (
        <motion.div
            className="absolute inset-0 z-[60] flex items-center justify-center bg-slate-900/40 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
        >
            <div className="w-[650px] rounded-2xl border border-slate-200 bg-white shadow-card-lg overflow-hidden flex flex-col">

                {/* Header */}
                <div className="bg-slate-50 p-3 border-b border-slate-200 flex justify-between items-center text-xs font-semibold">
                    <div className="flex gap-4 text-nobody-mint">
                        <span>💰 Shielded Wallet</span>
                        <span className="text-slate-500">🔒 ZK-Compression: On</span>
                    </div>
                    <span className="text-nobody-violet">⚡ Mesh Ready</span>
                </div>

                {/* Main Content */}
                <div className="p-6 space-y-6">

                    {/* Balance Section */}
                    <div className="rounded-xl border border-slate-200 bg-slate-50 p-4 space-y-2">
                        <div className="flex items-center justify-between">
                            <span className="text-slate-500 text-sm">Available Balance</span>
                            <div
                                className="cursor-pointer"
                                onMouseEnter={() => setRevealBalance(true)}
                                onMouseLeave={() => setRevealBalance(false)}
                            >
                                {revealBalance ? (
                                    <span className="text-nobody-mint font-bold text-xl tracking-wide">
                                        {balance}
                                    </span>
                                ) : (
                                    <span className="text-slate-300 font-bold text-xl tracking-widest animate-pulse">
                                        ████████
                                    </span>
                                )}
                            </div>
                        </div>
                        <div className="text-[11px] text-slate-400 flex items-center gap-2">
                            (Hover to reveal)
                            <span className="text-nobody-mint">All funds are ZK-proven & snapshot updated (2h ago)</span>
                        </div>
                    </div>

                    {/* Proof Generator */}
                    <div className="space-y-2">
                        <div className="text-slate-400 text-xs font-semibold border-b border-slate-100 pb-1 w-max">
                            🛡️ Proof Generator
                        </div>
                        <ul className="text-xs space-y-1 text-slate-500">
                            <li>Valid for Mesh Trades: <span className="text-nobody-mint font-semibold">Yes</span></li>
                            <li>Last Integrity Check: <span className="text-nobody-mint font-semibold">Pass ✅</span></li>
                        </ul>
                    </div>

                    {/* Pending Intents */}
                    <div className="space-y-2">
                        <div className="text-slate-400 text-xs font-semibold border-b border-slate-100 pb-1 w-max">
                            Pending Intents (Locked Funds)
                        </div>
                        <ul className="text-xs space-y-2 text-slate-500">
                            {pendingIntents.map((intent) => (
                                <li key={intent.id} className="flex justify-between border-b border-slate-100 pb-1 last:border-0">
                                    <span>Intent <span className="text-slate-900 font-medium">{intent.id}</span>: {intent.amount}</span>
                                    <span className="text-slate-400 italic">({intent.reason})</span>
                                </li>
                            ))}
                        </ul>
                    </div>
                </div>

                {/* Footer Actions */}
                <div className="bg-slate-50 border-t border-slate-200 p-4 flex justify-between items-center text-xs">
                    <div className="flex gap-3">
                        <button className="bg-white hover:bg-slate-100 text-slate-700 px-3 py-2 rounded-lg border border-slate-200 transition-colors font-semibold">
                            Update Snapshot
                        </button>
                        <button className="bg-nobody-mint-soft hover:bg-nobody-mint hover:text-white text-nobody-mint px-3 py-2 rounded-lg transition-colors font-semibold">
                            Add Funds
                        </button>
                    </div>
                    <button
                        onClick={onClose}
                        className="text-slate-400 hover:text-red-500 transition-colors font-semibold flex items-center gap-1"
                    >
                        🗑️ Shred Proofs
                    </button>
                </div>

            </div>
        </motion.div>
    );
};
