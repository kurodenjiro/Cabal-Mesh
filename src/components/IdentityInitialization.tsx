import React, { useState } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";

interface IdentityInitializationProps {
    onComplete: () => void;
    onBack: () => void;
}

export const IdentityInitialization: React.FC<IdentityInitializationProps> = ({ onComplete, onBack }) => {
    const [mode, setMode] = useState<"create" | "import">("create");
    const [alias, setAlias] = useState("");
    const [step, setStep] = useState<"form" | "generating">("form");
    const [progress, setProgress] = useState(0);

    const handleCreate = async () => {
        console.log("🖱️ Generate Identity Clicked");
        setStep("generating");
        try {
            console.log("⚡ Invoking generate_new_identity...");
            // Pass the alias state to the backend
            await invoke("generate_new_identity", { alias: alias || "Anonymous Fox" });
            console.log("✅ Identity Generated");
            setProgress(100);
            setTimeout(onComplete, 1500);
        } catch (e) {
            console.error("❌ Generation Failed:", e);
            alert("Generation Failed: " + e);
            setStep("form");
        }
    };

    // Auto-generate alias on mount
    React.useEffect(() => {
        const adjectives = ["Silent", "Neon", "Hidden", "Digital", "Quantum", "Shadow"];
        const nouns = ["Fox", "Ghost", "Specter", "Wraith", "Signal", "Node"];
        const randomAlias = `${adjectives[Math.floor(Math.random() * adjectives.length)]} ${nouns[Math.floor(Math.random() * nouns.length)]}`;
        setAlias(randomAlias);
    }, []);

    return (
        <motion.div
            className="absolute inset-0 z-[60] flex items-center justify-center bg-slate-900/40 backdrop-blur-sm text-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
        >
            <div className="w-[600px] rounded-2xl border border-slate-200 bg-white shadow-card-lg relative overflow-hidden">

                {/* Header */}
                <div className="bg-slate-50 px-5 py-3 border-b border-slate-200 flex justify-between items-center text-xs">
                    <span className="text-slate-900 font-semibold tracking-wide">🛡️ Identity Creation</span>
                    <span className="text-slate-400">Mode: {mode === "create" ? "New" : "Import"}</span>
                    <span className="text-nobody-mint font-medium">Security: High</span>
                </div>

                <div className="p-8 space-y-8">

                    {/* Mode Selection */}
                    <div className="flex gap-4">
                        <button
                            onClick={() => setMode("create")}
                            disabled={step === "generating"}
                            className={`flex-1 rounded-xl border p-4 text-left transition-colors group ${mode === "create" ? "border-nobody-mint bg-nobody-mint-soft/40" : "border-slate-200 hover:border-slate-300"} ${step === "generating" ? "opacity-50 cursor-not-allowed" : ""}`}
                        >
                            <div className={`font-semibold mb-1 ${mode === "create" ? "text-nobody-mint" : "text-slate-600 group-hover:text-slate-900"}`}>🆕 Generate New Identity</div>
                            <div className="text-xs text-slate-400">(Generate fresh ZK-keys)</div>
                        </button>
                    </div>

                    <div className="border-t border-slate-100" />

                    <div className={`space-y-6 ${step === "generating" ? "opacity-50 pointer-events-none" : ""}`}>
                        <div className="space-y-2">
                            <div className="text-slate-500 text-xs font-semibold">Assign Local Alias</div>
                            <input
                                type="text"
                                value={alias}
                                onChange={(e) => setAlias(e.target.value)}
                                placeholder="Name: The Clueless Fox"
                                disabled={step === "generating"}
                                className="w-full bg-slate-50 border border-slate-200 rounded-xl p-3 text-slate-900 focus:border-nobody-mint focus:outline-none focus:bg-white placeholder-slate-400 disabled:bg-slate-100 disabled:text-slate-400 transition-colors"
                            />
                            <div className="text-[11px] text-slate-400 text-right">(Local only — not leaked to Mesh)</div>
                        </div>

                        <div className="space-y-2">
                            <div className="text-slate-500 text-xs font-semibold">Set Master Password</div>
                            <input
                                type="password"
                                placeholder="Password"
                                disabled={step === "generating"}
                                className="w-full bg-slate-50 border border-slate-200 rounded-xl p-3 text-slate-900 focus:border-nobody-mint focus:outline-none focus:bg-white placeholder-slate-400 disabled:bg-slate-100 transition-colors"
                            />
                            <input
                                type="text"
                                placeholder="Hint: My first hardware..."
                                disabled={step === "generating"}
                                className="w-full bg-slate-50 border border-slate-200 rounded-xl p-3 text-slate-500 focus:border-slate-300 focus:outline-none placeholder-slate-400 text-xs disabled:bg-slate-100 transition-colors"
                            />
                        </div>
                    </div>

                    <div className="border-t border-slate-100" />

                    {/* Progression / Action */}
                    {step === "generating" ? (
                        <div className="space-y-4">
                            <div className="text-nobody-mint font-semibold animate-pulse">⚙️ Generating crypto assets...</div>
                            <ul className="text-xs space-y-2 text-slate-500">
                                <li className="flex items-center justify-between">
                                    <span>Creating Avalanche Keypair...</span>
                                    <span className={progress > 30 ? "text-nobody-mint font-medium" : "text-slate-300"}>{progress > 30 ? "Done" : "..."}</span>
                                </li>
                                <li className="flex items-center justify-between">
                                    <span>Deriving Mesh PeerID...</span>
                                    <span className={progress > 60 ? "text-nobody-mint font-medium" : "text-slate-300"}>{progress > 60 ? "Done" : "..."}</span>
                                </li>
                                <li className="flex items-center justify-between">
                                    <span>Setting AES-256 Vault...</span>
                                    <span className={progress >= 100 ? "text-nobody-mint font-medium" : "text-slate-300"}>{progress >= 100 ? "Done" : "..."}</span>
                                </li>
                            </ul>

                            {progress >= 100 && (
                                <motion.div
                                    initial={{ opacity: 0 }}
                                    animate={{ opacity: 1 }}
                                    className="bg-nobody-mint-soft/50 border border-nobody-mint/20 rounded-xl p-3 text-xs text-slate-600 mt-4"
                                >
                                    <span className="text-nobody-mint font-semibold">Agent:</span> "Your local identity is ready. Remember: Master Pass is the ONLY way to unlock your ZK-Proofs."
                                </motion.div>
                            )}

                            {progress >= 100 && (
                                <button
                                    onClick={onComplete}
                                    className="w-full bg-nobody-mint text-white font-semibold py-3 rounded-xl hover:bg-emerald-700 transition-colors mt-4"
                                >
                                    🚀 Initialize & Shred RAM
                                </button>
                            )}
                        </div>
                    ) : (
                        <div className="flex gap-4">
                            <button
                                onClick={handleCreate}
                                className="flex-1 bg-nobody-mint text-white font-semibold py-3 rounded-xl hover:bg-emerald-700 transition-colors"
                            >
                                {mode === "create" ? "Generate Identity" : "Restore from Seed"}
                            </button>
                            <button
                                onClick={onBack}
                                className="px-6 rounded-xl border border-slate-200 text-slate-500 hover:text-slate-900 hover:border-slate-300 transition-colors font-semibold"
                            >
                                ← Back
                            </button>
                        </div>
                    )}

                </div>
            </div>
        </motion.div>
    );
};
