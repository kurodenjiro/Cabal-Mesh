import React, { useState } from "react";
import { motion } from "framer-motion";

export const Feed: React.FC = () => {
    const [isExpanded, setIsExpanded] = useState(false);

    // Mock Items based on V2 Spec
    const feedItems = [
        {
            id: 1, type: "user", user: "User_C", meta: "500m Away - Mesh Direct",
            content: "Looking to buy used ESP32 components",
            fee: "10 AVAX",
            actions: ["Send Quote", "Direct Chat"]
        },
        {
            id: 2, type: "agent", user: "AI AGENT", meta: "Aggregated from Web",
            content: "Hot: Avalanche just released a Subnet tooling update...",
            actions: ["Read Summary", "Save Task"]
        },
        {
            id: 3, type: "nft", user: "BEYOND_US NFT", meta: "User_X - 2 Hops",
            content: "Just minted new collection on Avalanche",
            sub: "Compressed Vector Image",
            actions: ["❤️ 12", "⚡ Bid: 1.2 AVAX"]
        }
    ];

    return (
        <motion.div
            className="absolute bottom-0 left-0 right-0 bg-white border-t border-slate-200 shadow-card-lg transition-all duration-500 ease-in-out z-30 overflow-hidden flex flex-col"
            animate={{ height: isExpanded ? "60vh" : "6rem" }}
            initial={{ height: "6rem" }}
        >
            {/* Header: Search & Config */}
            <div
                className="h-12 shrink-0 flex items-center justify-between px-4 border-b border-slate-100 bg-slate-50 cursor-pointer"
                onClick={() => setIsExpanded(!isExpanded)}
            >
                <div className="flex gap-4 text-xs text-slate-400">
                    <span className="hover:text-slate-900 transition-colors">🔍 Search Intent...</span>
                </div>
                <div className={`text-slate-300 transform transition-transform ${isExpanded ? 'rotate-180' : ''}`}>▲</div>
            </div>

            {/* Notification / Alert Area */}
            <div className="bg-nobody-mint-soft/40 px-4 py-2 border-b border-slate-100 text-xs">
                <span className="text-nobody-mint font-semibold">3 new tasks matching profile</span>
                <span className="ml-2 text-slate-400 italic"> {"<- Priority Alert from Agent"}</span>
            </div>

            {/* List Content */}
            <div className="p-4 space-y-4 overflow-y-auto flex-1 bg-slate-50">
                {feedItems.map((item) => (
                    <div key={item.id} className="rounded-2xl border border-slate-200 bg-white shadow-card overflow-hidden relative">
                        {/* Header Line */}
                        <div className="bg-slate-50 px-3 py-1.5 flex justify-between items-center text-[11px] text-slate-500 border-b border-slate-100">
                            <span>
                                {item.type === 'user' && '👤'}
                                {item.type === 'agent' && '🤖'}
                                {item.type === 'nft' && '🖼️'}
                                <span className="ml-1 text-slate-900 font-semibold">{item.user}</span> ({item.meta})
                            </span>
                        </div>

                        {/* Content */}
                        <div className="p-3 text-sm text-slate-600">
                            {item.sub && <div className="text-xs text-slate-400 mb-1">{item.sub}</div>}
                            <div className="mb-2">"{item.content}"</div>
                            {item.fee && <div className="text-nobody-mint font-semibold text-xs mb-2">💰 Proposal: {item.fee}</div>}

                            {/* Actions */}
                            <div className="flex gap-2 mt-2 pt-2 border-t border-slate-100 border-dashed">
                                {item.actions.map((action, i) => (
                                    <button key={i} className="text-[11px] text-slate-500 hover:text-slate-900 border border-slate-200 hover:border-slate-300 px-2 py-1 rounded-lg transition-colors">
                                        {action}
                                    </button>
                                ))}
                            </div>
                        </div>
                    </div>
                ))}
            </div>
        </motion.div>
    );
};
