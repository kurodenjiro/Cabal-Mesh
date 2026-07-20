import React, { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";

interface RelayerStatusProps {
    isRelaying: boolean;
    onToggle: (enabled: boolean) => void;
}

export const RelayerStatus: React.FC<RelayerStatusProps> = ({ isRelaying, onToggle }) => {
    const [traffic, setTraffic] = useState("0 KB");
    const [earnings, setEarnings] = useState("0.0000 AVAX");
    const [activeConnections, setActiveConnections] = useState(0);

    // Simulate Relayer Activity
    useEffect(() => {
        if (!isRelaying) return;

        const interval = setInterval(() => {
            // Randomly simulate traffic spikes and earnings
            const trafficSpike = Math.random() > 0.7;
            if (trafficSpike) {
                setTraffic(prev => {
                    const current = parseFloat(prev.split(" ")[0]);
                    return (current + (Math.random() * 0.5)).toFixed(2) + " MB";
                });

                setEarnings(prev => {
                    const current = parseFloat(prev.split(" ")[0]);
                    return (current + 0.0005).toFixed(4) + " AVAX";
                });

                setActiveConnections(Math.floor(Math.random() * 5) + 1);
            } else {
                setActiveConnections(Math.max(1, activeConnections - 1));
            }
        }, 2000);

        return () => clearInterval(interval);
    }, [isRelaying, activeConnections]);

    return (
        <div className="rounded-2xl border border-slate-200 bg-nobody-charcoal shadow-card p-4 relative overflow-hidden">
            {/* Background Glow when Active */}
            <AnimatePresence>
                {isRelaying && (
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 0.06 }}
                        exit={{ opacity: 0 }}
                        className="absolute inset-0 bg-nobody-mint z-0"
                    />
                )}
            </AnimatePresence>

            <div className="relative z-10 flex flex-col gap-4">
                <div className="flex justify-between items-center">
                    <div className="flex items-center gap-2">
                        <div className={`w-2.5 h-2.5 rounded-full ${isRelaying ? "bg-nobody-mint animate-pulse" : "bg-slate-300"}`} />
                        <span className="text-slate-900 font-semibold tracking-wide text-sm">Relayer Mode</span>
                    </div>

                    {/* Toggle Switch */}
                    <button
                        onClick={() => onToggle(!isRelaying)}
                        className={`w-12 h-6 rounded-full p-1 transition-colors ${isRelaying ? "bg-nobody-mint" : "bg-slate-200"}`}
                    >
                        <motion.div
                            layout
                            className="w-4 h-4 rounded-full bg-nobody-charcoal shadow-md"
                            animate={{ x: isRelaying ? 24 : 0 }}
                        />
                    </button>
                </div>

                {isRelaying ? (
                    <div className="grid grid-cols-2 gap-4 pt-2">
                        <div>
                            <div className="text-slate-400 text-xs font-medium mb-1">Data Processed</div>
                            <div className="text-nobody-mint font-mono font-semibold">{traffic}</div>
                        </div>
                        <div>
                            <div className="text-slate-400 text-xs font-medium mb-1">Relay Earnings</div>
                            <div className="text-nobody-violet font-mono font-semibold">{earnings}</div>
                        </div>
                        <div className="col-span-2">
                            <div className="text-slate-400 text-xs font-medium mb-1">Active Hop Connections</div>
                            <div className="flex gap-1 h-2">
                                {[...Array(5)].map((_, i) => (
                                    <div
                                        key={i}
                                        className={`flex-1 rounded-full transition-colors duration-300 ${i < activeConnections ? "bg-nobody-mint" : "bg-slate-100"}`}
                                    />
                                ))}
                            </div>
                        </div>
                    </div>
                ) : (
                    <div className="text-slate-400 text-xs italic pt-2">
                        Enable Relayer Mode to earn incentives by forwarding mesh traffic.
                    </div>
                )}
            </div>
        </div>
    );
};
