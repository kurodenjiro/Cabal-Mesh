import React, { useState } from "react";
import { motion } from "framer-motion";

interface ServiceCreatorProps {
    onClose: () => void;
    onDeploy: (service: any) => void;
}

export const ServiceCreator: React.FC<ServiceCreatorProps> = ({ onClose, onDeploy }) => {
    const [prompt, setPrompt] = useState("");
    const [isAnalyzing, setIsAnalyzing] = useState(false);
    const [config, setConfig] = useState<any>(null);

    const handleAnalyze = () => {
        setIsAnalyzing(true);
        // Simulate AI Agent Configuration
        setTimeout(() => {
            setConfig({
                logic: "AI Generative Service (Local Stable Diffusion)",
                price: "0.1 AVAX (Fixed)",
                privacy: "Noir ZK-Proof Payment Verification (Enabled)",
                delivery: "Encrypted Mesh Relay (Enabled)",
                preview: "Anonymous AI Pixel Artist — 0.1 AVAX"
            });
            setIsAnalyzing(false);
        }, 2000);
    };

    return (
        <motion.div
            className="absolute inset-0 bg-slate-900/40 backdrop-blur-sm z-50 flex items-center justify-center p-8"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
        >
            <div className="w-full max-w-2xl rounded-2xl border border-slate-200 bg-white shadow-card-lg overflow-hidden flex flex-col">

                {/* Header */}
                <div className="bg-slate-50 p-3 border-b border-slate-200 flex justify-between items-center">
                    <span className="text-nobody-violet font-semibold tracking-wide text-sm">✨ Create New Service</span>
                    <div className="flex gap-4 text-xs">
                        <span className="text-slate-500">Mode: Provider</span>
                        <span className="text-nobody-mint font-medium">🛡️ Privacy: Shielded</span>
                    </div>
                </div>

                <div className="p-6 space-y-6">

                    {/* Prompt Input */}
                    <div className="space-y-2">
                        <label className="text-slate-500 text-xs font-semibold">Service Prompt</label>
                        <textarea
                            value={prompt}
                            onChange={(e) => setPrompt(e.target.value)}
                            onBlur={() => !config && prompt && handleAnalyze()}
                            placeholder='Example: "I want to sell an AI Pixel Art service for 0.1 AVAX per image..."'
                            className="w-full h-24 bg-slate-50 border border-slate-200 rounded-xl p-3 text-sm text-slate-900 focus:border-nobody-mint focus:bg-white outline-none resize-none transition-colors"
                        />
                    </div>

                    {/* Agent Config Output */}
                    {(isAnalyzing || config) && (
                        <div className="border-l-2 border-nobody-mint pl-4 space-y-2 transition-all">
                            <div className="text-nobody-mint font-semibold text-xs animate-pulse">
                                🤖 Agent configuring...
                            </div>

                            {config ? (
                                <div className="text-sm space-y-1 text-slate-600">
                                    <div>Logic: <span className="text-slate-900 font-medium">{config.logic}</span></div>
                                    <div>Price: <span className="text-slate-900 font-medium">{config.price}</span></div>
                                    <div>Privacy: <span className="text-nobody-mint font-medium">{config.privacy}</span></div>
                                    <div>Delivery: <span className="text-nobody-mint font-medium">{config.delivery}</span></div>
                                </div>
                            ) : (
                                <div className="text-slate-400 text-sm">Analyzing intent semantics...</div>
                            )}
                        </div>
                    )}

                    {/* Traits */}
                    {config && (
                        <div className="grid grid-cols-2 gap-3 border-t border-slate-100 pt-4 text-xs">
                            <div className="text-slate-500">🎨 Type: <span className="text-slate-900 font-medium">Digital Art</span></div>
                            <div className="text-slate-500">🔒 Access: <span className="text-slate-900 font-medium">Private (ZK-Gated)</span></div>
                            <div className="text-slate-500">⚡ Speed: <span className="text-slate-900 font-medium">Ultra (M4 Max)</span></div>
                            <div className="text-slate-500">💰 Payout: <span className="text-slate-900 font-medium">Instant</span></div>
                        </div>
                    )}

                    {/* Preview */}
                    {config && (
                        <div className="bg-nobody-violet-soft rounded-xl p-3 text-center text-sm text-nobody-violet font-semibold">
                            Preview listing: "{config.preview}"
                        </div>
                    )}
                </div>

                {/* Footer Actions */}
                <div className="border-t border-slate-200 p-4 flex justify-between bg-slate-50">
                    <button onClick={onClose} className="text-slate-400 hover:text-slate-700 transition-colors text-xs font-semibold">🗑️ Discard</button>
                    <div className="flex gap-3">
                        <button className="text-slate-500 hover:text-slate-900 transition-colors text-xs font-semibold border border-slate-200 rounded-lg px-3 py-2 bg-white">⚙️ Advanced Setup</button>
                        <button
                            onClick={() => onDeploy(config)}
                            disabled={!config}
                            className={`text-white text-xs font-semibold px-4 py-2 rounded-lg transition-colors ${config ? 'bg-nobody-mint hover:bg-emerald-700' : 'bg-slate-200 text-slate-400 cursor-not-allowed'}`}
                        >
                            🚀 Deploy to Mesh
                        </button>
                    </div>
                </div>

            </div>
        </motion.div>
    );
};
