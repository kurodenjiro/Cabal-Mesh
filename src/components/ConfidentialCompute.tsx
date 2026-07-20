import React, { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { CabalFigure } from "./icons/CabalFigure";

interface ConfidentialComputeProps {
    visible: boolean;
    onClose: () => void;
}

export const ConfidentialCompute: React.FC<ConfidentialComputeProps> = ({ visible, onClose }) => {
    const [lines, setLines] = useState<any[]>([]);

    useEffect(() => {
        if (visible) {
            setLines([]);
            const sequence = [
                { side: 'A', text: "I have a ZK-Proof of funds.", delay: 500 },
                { side: 'B', text: "I have the 'Clueless Fox' NFT.", delay: 1500 },
                { side: 'A', text: "My bid is 10 AVAX.", delay: 2500 },
                { side: 'B', text: "Market price is 16 AVAX.", delay: 3500 },
                { side: 'A', text: "Analyzing scarcity... Counter: 12.", delay: 4500 },
                { side: 'B', text: "Calculating ROI... Counter: 14.", delay: 5500 },
                { side: 'CENTER', text: "Final match found: 13.5 AVAX", delay: 7000 }
            ];

            sequence.forEach(({ side, text, delay }) => {
                setTimeout(() => {
                    setLines(prev => [...prev, { side, text }]);
                }, delay);
            });
        }
    }, [visible]);

    if (!visible) return null;

    return (
        <motion.div
            className="absolute inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
        >
            <div className="w-[800px] rounded-2xl border border-slate-200 bg-nobody-charcoal shadow-card-lg overflow-hidden flex flex-col relative h-[500px]">

                {/* Header */}
                <div className="bg-slate-50 p-4 border-b border-slate-200 flex justify-center text-xs font-semibold tracking-[0.1em] relative">
                    <span className="text-slate-900">Confidential Compute (MPC/FHE)</span>
                    <button onClick={onClose} className="absolute right-4 text-slate-400 hover:text-slate-700 top-4 transition-colors">✕</button>
                </div>

                {/* Main Arena */}
                <div className="flex-1 flex relative">

                    {/* Agent A Zone */}
                    <div className="w-1/2 p-6 border-r border-slate-100 flex flex-col items-start bg-nobody-mint-soft/20">
                        <div className="flex items-center gap-3 mb-8">
                            <div className="w-10 h-10 rounded-full bg-nobody-charcoal border border-nobody-mint flex items-center justify-center text-nobody-mint shadow-card">
                                <CabalFigure size={22} />
                            </div>
                            <div>
                                <div className="text-nobody-mint font-semibold text-sm">Agent A (Shark)</div>
                                <div className="text-[11px] text-slate-400">Local Brain</div>
                            </div>
                        </div>
                        <div className="space-y-4 w-full">
                            {lines.filter(l => l.side === 'A').map((l, i) => (
                                <motion.div key={i} initial={{ opacity: 0, x: -20 }} animate={{ opacity: 1, x: 0 }}>
                                    <div className="bg-nobody-charcoal border border-nobody-mint/30 shadow-card p-3 rounded-tr-xl rounded-bl-xl rounded-tl-xl text-xs text-slate-600">
                                        "{l.text}"
                                    </div>
                                </motion.div>
                            ))}
                        </div>
                    </div>

                    {/* Agent B Zone */}
                    <div className="w-1/2 p-6 flex flex-col items-end bg-nobody-violet-soft/20">
                        <div className="flex items-center gap-3 mb-8 flex-row-reverse text-right">
                            <div className="w-10 h-10 rounded-full bg-nobody-charcoal border border-nobody-violet flex items-center justify-center text-nobody-violet shadow-card">
                                <CabalFigure size={22} />
                            </div>
                            <div>
                                <div className="text-nobody-violet font-semibold text-sm">Agent B (Seller)</div>
                                <div className="text-[11px] text-slate-400">Mesh Node (Alpha-7)</div>
                            </div>
                        </div>
                        <div className="space-y-4 w-full flex flex-col items-end">
                            {lines.filter(l => l.side === 'B').map((l, i) => (
                                <motion.div key={i} initial={{ opacity: 0, x: 20 }} animate={{ opacity: 1, x: 0 }}>
                                    <div className="bg-nobody-charcoal border border-nobody-violet/30 shadow-card p-3 rounded-tl-xl rounded-br-xl rounded-tr-xl text-xs text-slate-600 text-right">
                                        "{l.text}"
                                    </div>
                                </motion.div>
                            ))}
                        </div>
                    </div>

                    {/* VS Badge */}
                    <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-nobody-charcoal border border-slate-200 rounded-full w-12 h-12 flex items-center justify-center font-bold text-slate-400 text-xs z-10 shadow-card-lg">
                        VS
                    </div>

                    {/* Match Result Overlay */}
                    {lines.some(l => l.side === 'CENTER') && (
                        <motion.div
                            initial={{ scale: 0.5, opacity: 0 }}
                            animate={{ scale: 1, opacity: 1 }}
                            className="absolute bottom-10 left-1/2 -translate-x-1/2 bg-nobody-violet text-nobody-ink px-6 py-3 font-semibold text-sm rounded-xl shadow-card-lg w-max"
                        >
                            {lines.find(l => l.side === 'CENTER').text}
                        </motion.div>
                    )}

                </div>
            </div>
        </motion.div>
    );
};
