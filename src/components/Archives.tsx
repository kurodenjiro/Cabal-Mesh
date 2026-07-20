import React from "react";
import { motion } from "framer-motion";

interface ArchivesProps {
    visible: boolean;
    onClose: () => void;
}

export const Archives: React.FC<ArchivesProps> = ({ visible, onClose }) => {
    if (!visible) return null;

    const history = [
        { id: "042a", item: "Clueless Fox #04", price: "13.5 AVAX", status: "SUCCESS", date: "2026-01-29" },
        { id: "9b1c", item: "Shadow Drive 10GB", price: "2.1 AVAX", status: "SHREDDED", date: "2026-01-28" },
        { id: "3f8x", item: "Mesh Compute Unit", price: "0.5 AVAX", status: "FAILED", date: "2026-01-28" },
    ];

    return (
        <motion.div
            className="absolute inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
        >
            <div className="w-[800px] rounded-2xl border border-slate-200 bg-nobody-charcoal shadow-card-lg p-6 relative">
                <button onClick={onClose} className="absolute top-4 right-4 text-slate-400 hover:text-slate-700 transition-colors">✕</button>

                <h2 className="text-xl text-slate-900 font-bold mb-6 border-b border-slate-100 pb-3 flex items-center gap-2">
                    <span className="text-nobody-violet">📂</span> Archives — Transaction Log
                </h2>

                <div className="space-y-2">
                    <div className="grid grid-cols-5 text-slate-400 text-xs font-semibold mb-2 px-3">
                        <div>ID</div>
                        <div className="col-span-2">Item</div>
                        <div>Price</div>
                        <div>Status</div>
                    </div>
                    {history.map((tx) => (
                        <div key={tx.id} className="grid grid-cols-5 text-sm p-3 rounded-xl bg-slate-50 border border-slate-100 hover:border-slate-200 transition-colors cursor-default">
                            <div className="text-slate-400">#{tx.id}</div>
                            <div className="col-span-2 text-slate-900 font-semibold">{tx.item}</div>
                            <div className="text-slate-600">{tx.price}</div>
                            <div className={`font-semibold ${tx.status === "SUCCESS" ? "text-nobody-mint" :
                                    tx.status === "SHREDDED" ? "text-slate-400 line-through" : "text-red-500"
                                }`}>
                                {tx.status}
                            </div>
                        </div>
                    ))}
                </div>

                <div className="mt-6 flex justify-between items-center text-[11px] text-slate-400">
                    <div>Total records: {history.length}</div>
                    <div className="animate-pulse">Auto-shred enabled</div>
                </div>
            </div>
        </motion.div>
    );
};
