import React, { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";

interface IntegrityShieldProps {
    visible: boolean;
    onComplete: () => void;
}

export const IntegrityShield: React.FC<IntegrityShieldProps> = ({ visible, onComplete }) => {
    const [progress, setProgress] = useState(0);
    const [step, setStep] = useState(0);
    const steps = [
        "Payload Signature (ShadowWire standard)",
        "ZK-Proof Validity (Noir verified)",
        "Content Hash Consistency",
        "Reputation Consensus (3/5 Peers)"
    ];

    useEffect(() => {
        if (visible) {
            setProgress(0);
            setStep(0);
            const interval = setInterval(() => {
                setProgress(p => {
                    if (p >= 100) {
                        clearInterval(interval);
                        setTimeout(onComplete, 1500); // Wait a bit before completing
                        return 100;
                    }
                    return p + 2;
                });
            }, 30);

            // Simulate step completion
            const stepInterval = setInterval(() => {
                setStep(s => (s < 4 ? s + 1 : s));
            }, 800);

            return () => {
                clearInterval(interval);
                clearInterval(stepInterval);
            }
        }
    }, [visible, onComplete]);

    if (!visible) return null;

    return (
        <AnimatePresence>
            <motion.div
                className="absolute inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
            >
                <div className="w-[600px] rounded-2xl border border-slate-200 bg-nobody-charcoal shadow-card-lg overflow-hidden p-6 relative">

                    {/* Header */}
                    <div className="flex justify-between items-center mb-6 border-b border-slate-100 pb-3">
                        <div className="flex gap-4">
                            <span className="text-nobody-mint font-semibold">🔒 Integrity Shield</span>
                            <span className="text-slate-400 text-xs">Sender: Nobody_42a8</span>
                        </div>
                        <span className="text-xs text-amber-400 animate-pulse font-semibold">⚡ Status: Verifying</span>
                    </div>

                    {/* Visual Flow Animation */}
                    <div className="flex justify-between items-center mb-8 px-4 text-[11px] text-slate-400 font-semibold">
                        <div className="border border-slate-200 rounded-lg p-2">Mesh Packet</div>
                        <div className="text-nobody-mint animate-pulse">{"→ Decrypting →"}</div>
                        <div className="border border-nobody-mint text-nobody-mint rounded-lg p-2">Noir ZK-Proof</div>
                        <div className="text-nobody-mint animate-pulse">{"→ Checking →"}</div>
                        <div className="border border-slate-200 rounded-lg p-2">Local Brain</div>
                    </div>

                    {/* Progress Bar */}
                    <div className="h-1.5 bg-slate-100 rounded-full w-full mb-6 overflow-hidden">
                        <motion.div
                            className="h-full bg-nobody-mint rounded-full"
                            style={{ width: `${progress}%` }}
                        />
                    </div>

                    {/* Checklist */}
                    <div className="space-y-3 mb-6">
                        <div className="text-slate-400 text-xs font-semibold border-b border-slate-100 pb-1 mb-2">
                            Checklist
                        </div>
                        {steps.map((label, i) => (
                            <motion.div
                                key={i}
                                className="flex justify-between items-center text-sm"
                                initial={{ opacity: 0, x: -10 }}
                                animate={{ opacity: step >= i ? 1 : 0.3, x: 0 }}
                                transition={{ duration: 0.3 }}
                            >
                                <div className="flex items-center gap-3">
                                    <span className="text-slate-400">{i + 1}.</span>
                                    <span className={`${step > i ? "text-slate-900" : "text-slate-400"}`}>{label}</span>
                                </div>
                                <div>
                                    {step > i && <span className="text-nobody-mint font-semibold text-xs">✅</span>}
                                    {step === i && <span className="text-amber-400 font-semibold text-xs animate-pulse">⏳</span>}
                                    {step < i && <span className="text-slate-300 font-semibold text-xs">...</span>}
                                </div>
                            </motion.div>
                        ))}
                    </div>

                    {/* Security Log */}
                    <div className="bg-slate-50 rounded-xl border border-slate-100 p-3 text-xs space-y-1 font-mono text-slate-500 h-24 overflow-y-auto">
                        <div className="text-slate-400 font-semibold mb-1 border-b border-slate-100 pb-1">🦈 Security Agent Log</div>
                        {step >= 1 && <div className="text-slate-600">"Detecting 2 relay hops. All headers stripped successfully."</div>}
                        {step >= 2 && <div className="text-slate-600">"Verifying ZK-Proof... Mathematical integrity confirmed."</div>}
                        {step >= 3 && <div className="text-red-400">Warning: Node_7b tried to append metadata. Blocked & Discarded.</div>}
                    </div>

                    {/* Footer Actions (Static for visual) */}
                    <div className="mt-4 flex gap-2">
                        <button className="flex-1 bg-slate-100 text-slate-400 rounded-xl py-2 text-xs font-semibold cursor-not-allowed">
                            Unlock & Execute
                        </button>
                    </div>

                    {progress >= 100 && (
                        <motion.div
                            initial={{ scale: 0.8, opacity: 0 }}
                            animate={{ scale: 1, opacity: 1 }}
                            className="absolute inset-0 bg-nobody-charcoal/90 flex items-center justify-center backdrop-blur-sm z-20 rounded-2xl"
                        >
                            <div className="text-nobody-mint text-lg font-bold tracking-wide border-2 border-nobody-mint rounded-xl p-4 bg-nobody-charcoal shadow-card-lg">
                                Data is pure & untouched
                            </div>
                        </motion.div>
                    )}

                </div>
            </motion.div>
        </AnimatePresence>
    );
};
