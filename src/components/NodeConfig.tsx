import React from "react";
import { motion } from "framer-motion";

interface NodeConfigProps {
    visible: boolean;
    onClose: () => void;
}

export const NodeConfig: React.FC<NodeConfigProps> = ({ visible, onClose }) => {
    const [tab, setTab] = React.useState<"node" | "agent">("node");

    if (!visible) return null;

    return (
        <motion.div
            className="absolute top-16 right-4 z-40"
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
        >
            <div className="w-80 bg-nobody-charcoal border border-slate-200 rounded-2xl shadow-card-lg text-xs overflow-hidden">
                {/* Tabs */}
                <div className="flex border-b border-slate-200">
                    <Tab label="Node Ops" active={tab === "node"} onClick={() => setTab("node")} />
                    <Tab label="Strategy" active={tab === "agent"} onClick={() => setTab("agent")} />
                    <button onClick={onClose} className="px-4 text-slate-400 hover:text-slate-700 border-l border-slate-200 transition-colors">✕</button>
                </div>

                <div className="p-4">
                    {tab === "node" ? (
                        <>
                            <div className="flex justify-between mb-4">
                                <span className="text-slate-900 font-semibold">Status: Active ●</span>
                                <span className="text-slate-400">Node: Alpha-7</span>
                            </div>
                            <div className="space-y-3 mb-6">
                                <Toggle label="Share Bandwidth (5 MB/s)" defaultChecked />
                                <Toggle label="Share Compute (Llama-3)" defaultChecked />
                                <Toggle label="Auto-Verify Transactions" defaultChecked />
                            </div>
                            <div className="bg-slate-50 rounded-xl p-3 border border-slate-100 text-center">
                                <div className="text-slate-400 mb-1 text-[11px] font-medium">Projected Rewards</div>
                                <div className="text-xl text-nobody-mint font-bold">0.0042 AVAX/kb</div>
                            </div>
                        </>
                    ) : (
                        <>
                            <div className="mb-4 bg-nobody-violet-soft p-2 rounded-lg text-center">
                                <span className="text-nobody-violet font-semibold">Mode: Aggressive</span>
                            </div>
                            <div className="space-y-4 text-slate-500">
                                <div className="flex justify-between">
                                    <span>Goal:</span>
                                    <span className="text-slate-900 font-medium">Profit Max</span>
                                </div>
                                <div className="flex justify-between">
                                    <span>Security:</span>
                                    <span className="text-slate-900 font-medium">2FA via x402</span>
                                </div>
                                <div className="text-[11px] bg-amber-950/40 text-amber-400 p-2 rounded-lg border border-amber-900/40">
                                    Note: This mode may cause you to miss time-sensitive tasks.
                                </div>

                                <div className="mt-4 pt-4 border-t border-slate-100 flex justify-between items-center text-[11px]">
                                    <span>Flexible</span>
                                    <div className="h-1 flex-1 mx-2 bg-slate-100 rounded-full relative">
                                        <div className="absolute right-0 top-1/2 -translate-y-1/2 w-3 h-3 bg-nobody-violet rounded-full shadow-card" />
                                    </div>
                                    <span>Aggressive</span>
                                </div>
                            </div>
                        </>
                    )}
                </div>

                <div className="p-3 border-t border-slate-200 bg-slate-50 text-center">
                    <button className="text-slate-500 hover:text-nobody-mint transition-colors text-[11px] font-semibold">
                        {tab === "node" ? "Update Configuration" : "Save Personality"}
                    </button>
                </div>
            </div>
        </motion.div>
    );
};

const Toggle = ({ label, defaultChecked }: { label: string, defaultChecked?: boolean }) => (
    <div className="flex justify-between items-center">
        <span className="text-slate-500">{label}</span>
        <input type="checkbox" defaultChecked={defaultChecked} className="accent-nobody-mint" />
    </div>
);

const Tab = ({ label, active, onClick }: { label: string, active: boolean, onClick: () => void }) => (
    <button
        onClick={onClick}
        className={`flex-1 py-3 text-center transition-colors font-semibold ${active ? 'bg-nobody-charcoal text-slate-900' : 'bg-slate-50 text-slate-400 hover:text-slate-600'}`}
    >
        {label}
    </button>
);
