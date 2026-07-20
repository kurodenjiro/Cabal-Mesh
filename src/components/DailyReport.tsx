import React, { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";

interface DailyReportProps {
    visible: boolean;
    onClose: () => void;
}

export const DailyReport: React.FC<DailyReportProps> = ({ visible, onClose }) => {
    const [typedText, setTypedText] = useState("");
    const fullText = "Hello partner. While you were away, I optimized your mesh interactions and secured 3 pending deals. Providing summary below...";

    useEffect(() => {
        if (visible) {
            setTypedText("");
            let i = 0;
            const interval = setInterval(() => {
                setTypedText(fullText.slice(0, i + 1));
                i++;
                if (i > fullText.length) clearInterval(interval);
            }, 30);
            return () => clearInterval(interval);
        }
    }, [visible]);

    if (!visible) return null;

    const today = new Date().toLocaleDateString('en-US', { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' });

    return (
        <AnimatePresence>
            <motion.div
                className="absolute inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm p-4"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
            >
                <div className="w-full max-w-3xl rounded-2xl border border-slate-200 bg-nobody-charcoal shadow-card-lg overflow-hidden relative">

                    {/* Header */}
                    <div className="bg-slate-50 p-4 border-b border-slate-200 flex justify-between items-center relative z-10">
                        <div className="flex items-center gap-3">
                            <div className="w-3 h-3 bg-nobody-mint rounded-full animate-pulse" />
                            <span className="text-slate-900 font-semibold tracking-wide text-lg">AI Partner Report</span>
                        </div>
                        <span className="text-slate-400 text-xs">{today}</span>
                    </div>

                    <div className="p-8 space-y-8 relative z-10">

                        {/* Intro Typewriter */}
                        <div className="min-h-[3rem] text-slate-600 text-sm border-l-2 border-nobody-mint pl-4">
                            {typedText}
                            <span className="animate-blink">_</span>
                        </div>

                        {/* Grid Layout */}
                        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">

                            {/* Overview Card */}
                            <motion.div
                                className="col-span-1 rounded-2xl bg-nobody-charcoal p-5 border border-slate-200 shadow-card hover:border-nobody-mint/50 transition-colors group"
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                transition={{ delay: 1.5 }}
                            >
                                <div className="text-slate-400 text-[11px] font-semibold mb-4 tracking-wide group-hover:text-nobody-mint transition-colors">
                                    Activity Overview
                                </div>
                                <ul className="space-y-3 text-sm">
                                    <li className="flex justify-between">
                                        <span className="text-slate-500">Scanned</span>
                                        <span className="text-slate-900 font-semibold">1,240</span>
                                    </li>
                                    <li className="flex justify-between">
                                        <span className="text-slate-500">Interactions</span>
                                        <span className="text-slate-900 font-semibold">5</span>
                                    </li>
                                    <li className="flex justify-between border-t border-slate-100 pt-2 mt-2">
                                        <span className="text-slate-500">Net Profit</span>
                                        <span className="text-nobody-mint font-semibold">+15.5 AVAX</span>
                                    </li>
                                </ul>
                            </motion.div>

                            {/* Opportunities Card */}
                            <motion.div
                                className="col-span-2 rounded-2xl bg-nobody-charcoal p-5 border border-slate-200 shadow-card hover:border-nobody-violet/50 transition-colors group"
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                transition={{ delay: 1.7 }}
                            >
                                <div className="text-slate-400 text-[11px] font-semibold mb-4 tracking-wide group-hover:text-nobody-violet transition-colors">
                                    Pending Approvals (2)
                                </div>

                                <div className="space-y-4">
                                    {/* Item 1 */}
                                    <div className="flex justify-between items-start group/item hover:bg-slate-50 p-2 rounded-xl -mx-2 transition-colors cursor-pointer">
                                        <div>
                                            <div className="text-slate-900 font-semibold text-sm">NFT Acquisition Request: "BEYOND #402"</div>
                                            <div className="text-slate-400 text-xs mt-1">
                                                User_D • Offer: 40 AVAX <span className="text-nobody-mint ml-2">(Auto-Haggled from 35)</span>
                                            </div>
                                        </div>
                                        <div className="flex gap-2 opacity-0 group-hover/item:opacity-100 transition-opacity">
                                            <button className="bg-nobody-mint text-white rounded-lg px-3 py-1 text-[11px] font-semibold hover:bg-emerald-700">Accept</button>
                                            <button className="border border-slate-200 text-slate-500 rounded-lg px-3 py-1 text-[11px] font-semibold hover:text-slate-900">Ignore</button>
                                        </div>
                                    </div>

                                    {/* Item 2 */}
                                    <div className="flex justify-between items-start group/item hover:bg-slate-50 p-2 rounded-xl -mx-2 transition-colors cursor-pointer">
                                        <div>
                                            <div className="text-slate-900 font-semibold text-sm">Delivery Relay Task</div>
                                            <div className="text-slate-400 text-xs mt-1">
                                                Route: Home {'->'} Office • Reward: 0.5 AVAX
                                            </div>
                                        </div>
                                        <div className="flex gap-2 opacity-0 group-hover/item:opacity-100 transition-opacity">
                                            <button className="bg-nobody-mint text-white rounded-lg px-3 py-1 text-[11px] font-semibold hover:bg-emerald-700">Accept</button>
                                        </div>
                                    </div>
                                </div>
                            </motion.div>

                        </div>

                        {/* Security Alert (Conditional) */}
                        <motion.div
                            className="rounded-xl border border-red-900/40 bg-red-950/40 p-3 flex items-center justify-between"
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            transition={{ delay: 2.5 }}
                        >
                            <div className="flex items-center gap-3">
                                <span className="text-red-400 text-lg">🛡️</span>
                                <span className="text-red-400 text-xs">Blocked 2 rogue nodes attempting signature replay.</span>
                            </div>
                            <button className="text-red-400 text-[11px] underline hover:text-red-300">View Logs</button>
                        </motion.div>

                    </div>

                    {/* Footer Input */}
                    <div className="bg-slate-50 ps-4 pe-2 py-3 border-t border-slate-200 flex items-center gap-3 relative z-10">
                        <span className="text-nobody-mint text-xs font-semibold animate-pulse">{'>'}_</span>
                        <input
                            type="text"
                            placeholder="Command agent..."
                            className="bg-transparent border-none outline-none text-slate-900 text-xs flex-1 placeholder-slate-400"
                            autoFocus
                            onKeyDown={(e) => {
                                if (e.key === 'Enter') {
                                    // Handle command (mock)
                                    onClose();
                                }
                            }}
                        />
                        <button
                            onClick={onClose}
                            className="bg-nobody-violet-soft hover:bg-nobody-violet hover:text-white text-nobody-violet rounded-lg px-4 py-2 text-[11px] font-semibold transition-all whitespace-nowrap"
                        >
                            Acknowledge
                        </button>
                    </div>

                </div>
            </motion.div>
        </AnimatePresence>
    );
};
