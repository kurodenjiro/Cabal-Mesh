import React, { useState, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { useRelayStats } from "../hooks/useRelayStats";
import relayIcon from "../assets/icons/icon_relay.png";

interface RelayerStatusCompactProps {
    isRelaying: boolean;
    onToggle: (enabled: boolean) => void;
    peerCount?: number;
}

interface RelayedTxRecord {
    summary: string;
    tx_hash: string;
    reward_avax: string;
    relayed_at: string;
}

/**
 * A small always-visible Relay Mode control for the main screen. Detail
 * stats are hidden behind an (i) info button as a popup instead of always
 * taking up space, so the outer screen stays uncluttered.
 */
export const RelayerStatusCompact: React.FC<RelayerStatusCompactProps> = ({ isRelaying, onToggle, peerCount = 0 }) => {
    const [showInfo, setShowInfo] = useState(false);
    const { traffic, earnings } = useRelayStats(isRelaying);
    const activeConnections = Math.min(peerCount, 5);
    const [history, setHistory] = useState<RelayedTxRecord[]>([]);

    const loadHistory = useCallback(() => {
        invoke<RelayedTxRecord[]>("get_relayed_history")
            .then(setHistory)
            .catch((e) => console.error("Failed to load relay history:", e));
    }, []);

    useEffect(() => {
        if (showInfo) loadHistory();
    }, [showInfo, loadHistory]);

    const totalReward = history.reduce((sum, h) => sum + (parseFloat(h.reward_avax) || 0), 0);

    return (
        <div className="relative">
            <div className="h-8 flex items-center gap-2 bg-nobody-charcoal pixel-corners-sm px-3 border border-nobody-primary/20 shadow-card">
                <div className={`w-2 h-2 rounded-full ${isRelaying ? "bg-nobody-primary animate-pulse" : "bg-slate-300"}`} />
                <span className="text-[10px] text-slate-500 font-medium flex items-center gap-1">
                    <img src={relayIcon} alt="" draggable={false} style={{ width: 12, height: 14, imageRendering: "pixelated" }} />
                    RELAY
                </span>
                <button
                    onClick={() => onToggle(!isRelaying)}
                    className={`w-8 h-4 rounded-full p-0.5 transition-colors ${isRelaying ? "bg-nobody-primary" : "bg-slate-200"}`}
                >
                    <motion.div layout className="w-3 h-3 rounded-full bg-nobody-charcoal shadow-md" animate={{ x: isRelaying ? 16 : 0 }} />
                </button>
                <button
                    onClick={() => setShowInfo(!showInfo)}
                    className="w-4 h-4 rounded-full border border-slate-300 text-slate-400 hover:text-nobody-primary hover:border-nobody-primary text-[9px] flex items-center justify-center transition-colors"
                >
                    i
                </button>
            </div>

            <AnimatePresence>
                {showInfo && (
                    <motion.div
                        initial={{ opacity: 0, y: -4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -4 }}
                        className="absolute top-10 right-0 w-64 pixel-corners-sm bg-nobody-charcoal border border-nobody-primary/30 shadow-card-lg p-3 z-50 text-xs"
                    >
                        {isRelaying ? (
                            <div className="space-y-2">
                                <div className="flex justify-between"><span className="text-slate-400">Data Processed</span><span className="text-nobody-primary font-mono font-semibold">{traffic}</span></div>
                                <div className="flex justify-between"><span className="text-slate-400">Relay Earnings</span><span className="text-nobody-gold font-mono font-semibold">{earnings}</span></div>
                                <div className="flex justify-between"><span className="text-slate-400">Connections</span><span className="text-slate-900 font-semibold">{activeConnections}</span></div>
                            </div>
                        ) : (
                            <div className="text-slate-500">Turn on Relay to help forward Mesh traffic and earn small AVAX rewards.</div>
                        )}

                        <div className="border-t border-slate-100 mt-3 pt-2">
                            <div className="flex justify-between mb-1">
                                <span className="text-slate-400">Helped relay for others</span>
                                <span className="text-slate-900 font-semibold">{history.length}</span>
                            </div>
                            {history.length > 0 && (
                                <>
                                    <div className="flex justify-between mb-2">
                                        <span className="text-slate-400">Est. reward earned</span>
                                        <span className="text-nobody-gold font-mono font-semibold">{totalReward.toFixed(6)} AVAX</span>
                                    </div>
                                    <div className="space-y-1.5 max-h-32 overflow-y-auto">
                                        {history.slice(-5).reverse().map((h, i) => (
                                            <div key={i} className="text-[11px] text-slate-500 truncate" title={h.tx_hash}>
                                                ✓ {h.summary} <span className="text-slate-400">({h.reward_avax} AVAX)</span>
                                            </div>
                                        ))}
                                    </div>
                                    <div className="text-[10px] text-slate-400 mt-2 italic">
                                        Estimate only — no on-chain relayer payout yet.
                                    </div>
                                </>
                            )}
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
};

export default RelayerStatusCompact;
