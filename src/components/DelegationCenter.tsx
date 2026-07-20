import React, { useState } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";

interface DelegationCenterProps {
    visible: boolean;
    onComplete: () => void; // Called after successful delegation
    onCancel: () => void;
}

export const DelegationCenter: React.FC<DelegationCenterProps> = ({ visible, onComplete, onCancel }) => {
    const [step, setStep] = useState<"idle" | "signing">("idle");
    const [progress, setProgress] = useState(0);

    const handleSignAndDelegate = async () => {
        setStep("signing");

        // Simulating the sequence from the ASCII art status
        // 1. Generating Ephemeral Keypair
        setTimeout(() => setProgress(30), 500);

        // 2. Initializing Instant Session
        setTimeout(() => setProgress(60), 1200);

        // 3. Signing (Real backend call)
        setTimeout(async () => {
            try {
                await invoke("enable_instant_session");
                setProgress(100);
                setTimeout(onComplete, 1000); // Wait a bit then close
            } catch (e) {
                console.error("Delegation Failed", e);
                alert("Delegation Failed: " + e);
                setStep("idle");
                setProgress(0);
            }
        }, 2000);
    };

    if (!visible) return null;

    return (
        <motion.div
            className="absolute inset-0 z-[60] flex items-center justify-center bg-slate-900/40 backdrop-blur-sm text-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
        >
            <div className="w-[650px] rounded-2xl border border-slate-200 bg-white shadow-card-lg relative flex flex-col overflow-hidden">

                {/* Header */}
                <div className="bg-slate-50 px-5 py-3 border-b border-slate-200 flex justify-between items-center text-xs">
                    <span className="text-slate-900 font-semibold tracking-wide">🛡️ Delegation Center</span>
                    <span className="text-slate-400">Identity: Clueless Fox</span>
                    <span className="text-nobody-mint font-medium">Engine: Instant Session</span>
                </div>

                <div className="p-8 space-y-6">

                    {/* Agent Authority Setup */}
                    <div className="space-y-2">
                        <div className="text-nobody-mint font-semibold text-xs tracking-wide">🗝️ Agent Authority Setup</div>
                        <div className="text-slate-500 text-xs leading-relaxed border-l-2 border-slate-200 pl-3">
                            You are about to authorize your AI Agent to negotiate and sign transactions on your behalf using an Instant Session key.
                        </div>
                    </div>

                    <div className="border-t border-slate-100" />

                    {/* Delegation Limits */}
                    <div className="space-y-4">
                        <div className="text-slate-400 font-semibold text-xs tracking-wide mb-3">📊 Delegation Limits</div>

                        <div className="grid grid-cols-2 gap-4 text-xs">
                            <div className="bg-slate-50 rounded-xl p-3 border border-slate-200 flex justify-between items-center text-slate-500">
                                <span>Max Spending Limit</span>
                                <span className="text-slate-900 font-semibold">5.00 AVAX</span>
                            </div>
                            <div className="bg-slate-50 rounded-xl p-3 border border-slate-200 flex justify-between items-center text-slate-500">
                                <span>Session Duration</span>
                                <span className="text-slate-900 font-semibold">24 Hours</span>
                            </div>
                        </div>

                        <div className="bg-slate-50 rounded-xl p-3 border border-slate-200 text-xs flex justify-between items-center text-slate-500">
                            <span>Allowed Protocols</span>
                            <span className="text-nobody-mint font-semibold">Instant Session, Private Swap, Starpay</span>
                        </div>
                    </div>

                    {/* Security Override */}
                    <div className="space-y-2">
                        <div className="text-slate-400 font-semibold text-xs tracking-wide mb-2">🔒 Security Override</div>
                        <div className="space-y-1.5 text-xs text-slate-500">
                            <div className="flex items-center gap-2">
                                <span className="text-nobody-mint">✓</span>
                                <span>Auto-terminate session if Mesh connection is lost.</span>
                            </div>
                            <div className="flex items-center gap-2">
                                <span className="text-nobody-mint">✓</span>
                                <span>Require Master Pass for transactions {">"} 1.0 AVAX.</span>
                            </div>
                        </div>
                    </div>

                    <div className="border-t border-slate-100" />

                    {/* Delegation Status (Dynamic) */}
                    <div className="bg-slate-50 rounded-xl p-4 border border-slate-200 space-y-2 relative overflow-hidden">
                        <div className="text-slate-400 font-semibold text-xs tracking-wide mb-2">⚙️ Delegation Status</div>

                        <ul className="text-xs space-y-1.5 text-slate-500">
                            <li className="flex justify-between">
                                <span>Generating Ephemeral Keypair...</span>
                                <span className={progress >= 30 ? "text-nobody-mint font-medium" : "text-slate-300"}>{progress >= 30 ? "Done" : "..."}</span>
                            </li>
                            <li className="flex justify-between">
                                <span>Initializing Instant Session...</span>
                                <span className={progress >= 60 ? "text-nobody-mint font-medium" : "text-slate-300"}>{progress >= 60 ? "Ready" : "..."}</span>
                            </li>
                            <li className="flex justify-between">
                                <span>Waiting for Owner Signature...</span>
                                <span className={progress >= 100 ? "text-nobody-mint font-medium" : "text-slate-300"}>{progress >= 100 ? "Signed" : step === "signing" ? "Pending" : "Waiting"}</span>
                            </li>
                        </ul>

                        <div className="mt-4 border-l-2 border-nobody-mint pl-3 py-1 text-xs text-slate-500">
                            <span className="text-nobody-mint font-semibold">Agent:</span> "I will manage your intents within these bounds. Once authorized, I can trade even while you are away."
                        </div>
                    </div>

                    {/* Actions */}
                    <div className="flex gap-4 pt-2">
                        <button
                            onClick={handleSignAndDelegate}
                            disabled={step === "signing"}
                            className={`flex-1 bg-nobody-mint text-white font-semibold py-3 rounded-xl hover:bg-emerald-700 transition-colors ${step === "signing" ? "opacity-70 cursor-wait" : ""}`}
                        >
                            {step === "signing" ? "✍️ Signing..." : "✍️ Sign & Delegate"}
                        </button>
                        <button
                            onClick={onCancel}
                            disabled={step === "signing"}
                            className="px-6 rounded-xl border border-slate-200 text-slate-500 hover:text-slate-900 hover:border-slate-300 transition-colors font-semibold disabled:opacity-50"
                        >
                            ← Cancel
                        </button>
                    </div>

                </div>
            </div>
        </motion.div>
    );

};
