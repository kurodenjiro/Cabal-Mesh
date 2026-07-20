import React from "react";
import { motion } from "framer-motion";

interface DealNotificationProps {
    visible: boolean;
    onClose: () => void;
    onAccept?: () => void;
}

export const DealNotification: React.FC<DealNotificationProps> = ({ visible, onClose, onAccept }) => {
    if (!visible) return null;

    return (
        <motion.div
            className="absolute top-20 right-8 z-[70] w-[400px]"
            initial={{ opacity: 0, x: 50 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 50 }}
        >
            <div className="border border-l-4 border-l-nobody-mint border-slate-200 bg-white rounded-2xl shadow-card-lg overflow-hidden">

                {/* Header */}
                <div className="bg-slate-50 p-3 border-b border-slate-200 flex justify-between items-center">
                    <div className="flex items-center gap-2">
                        <span className="animate-pulse text-lg">🔔</span>
                        <span className="text-slate-900 text-xs font-semibold tracking-wide">Notification</span>
                    </div>
                    <span className="text-[11px] text-slate-400">just now</span>
                </div>

                {/* Body */}
                <div className="p-4 space-y-4 relative">
                    <div className="text-slate-900 text-sm font-medium border-l-2 border-slate-200 pl-3 py-1">
                        "Agent found a matching Buyer for 'Clueless Fox #04'"
                    </div>

                    <div className="bg-slate-50 rounded-xl p-3 border border-slate-100 space-y-2 text-xs">
                        <div className="text-slate-400 font-semibold mb-2 border-b border-slate-100 pb-1">
                            📈 Deal Analysis
                        </div>
                        <div className="flex justify-between">
                            <span className="text-slate-500">Buyer</span>
                            <span className="text-slate-900 font-semibold">Verified Nobody <span className="text-nobody-mint">(Rep: 99%)</span></span>
                        </div>
                        <div className="flex justify-between">
                            <span className="text-slate-500">Price</span>
                            <span className="text-slate-900 font-semibold">13.5 AVAX <span className="text-slate-400 font-normal">(Floor: 12.0)</span></span>
                        </div>
                        <div className="flex justify-between border-t border-slate-100 pt-2 mt-1">
                            <span className="text-slate-500">Net Profit</span>
                            <span className="text-nobody-mint font-semibold">+1.5 AVAX</span>
                        </div>
                    </div>

                    {/* Actions */}
                    <div className="space-y-2">
                        <div className="text-slate-400 text-[11px] font-semibold tracking-wide mb-1">
                            Action
                        </div>
                        <div className="grid grid-cols-2 gap-2">
                            <button
                                onClick={onAccept}
                                className="bg-nobody-mint text-white font-semibold py-2 text-xs rounded-lg hover:bg-emerald-700 transition-colors flex items-center justify-center gap-1"
                            >
                                <span>✔ Accept & Mint</span>
                            </button>
                            <button
                                onClick={onClose}
                                className="bg-white text-slate-400 font-semibold py-2 text-xs rounded-lg hover:bg-red-50 hover:text-red-500 transition-colors border border-slate-200 hover:border-red-200"
                            >
                                ✕ Reject
                            </button>
                        </div>
                    </div>

                </div>
            </div>
        </motion.div>
    );
};
