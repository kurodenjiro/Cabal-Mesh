import React from "react";
import { motion } from "framer-motion";

interface IntentComposerProps {
    intent: string;
    setIntent: (val: string) => void;
    priceCeiling: string;
    setPriceCeiling: (val: string) => void;
    onSubmit: () => void;
    /** Whether any search is currently in flight — shown as a small badge only;
     * typing and submitting stay enabled so multiple intents can be queued. */
    isProcessing: boolean;
}

export const IntentComposer: React.FC<IntentComposerProps> = ({ intent, setIntent, priceCeiling, setPriceCeiling, onSubmit, isProcessing }) => {
    return (
        <div className="absolute bottom-6 left-6 w-[440px] z-20 flex flex-col items-start gap-2">
            <div className={`w-full relative bg-nobody-charcoal border transition-all duration-300 shadow-card-lg pixel-corners ${isProcessing ? 'border-nobody-primary' : 'border-nobody-gold/40'} p-1.5 flex items-stretch gap-1.5`}>

                {/* Processing Spinner — informational only, doesn't block new submissions */}
                {isProcessing && (
                    <div className="absolute -top-3 -right-3 w-6 h-6 bg-nobody-charcoal border border-nobody-primary pixel-corners-sm flex items-center justify-center z-10 animate-spin shadow-card">
                        <div className="w-2 h-2 bg-nobody-primary" />
                    </div>
                )}

                <div className="flex-1 relative">
                    <input
                        type="text"
                        value={intent}
                        onChange={(e) => setIntent(e.target.value)}
                        onKeyDown={(e) => {
                            if (e.key === "Enter") {
                                console.log("Enter Key Pressed");
                                onSubmit();
                            }
                        }}
                        placeholder="> describe your intent_"
                        className="w-full bg-slate-50 text-nobody-primary p-3 outline-none text-sm placeholder-slate-500 focus:bg-nobody-charcoal transition-colors pr-16 font-mono"
                    />

                    {/* Enter Badge / Submit Button */}
                    <button
                        onClick={() => {
                            console.log("Submit Button Clicked");
                            onSubmit();
                        }}
                        className="absolute right-2 top-1/2 -translate-y-1/2 text-[10px] font-pixel text-slate-500 hover:text-nobody-primary border border-slate-300 hover:border-nobody-primary bg-nobody-charcoal px-2 py-1 pixel-corners-sm transition-all"
                    >
                        [ENT]
                    </button>

                    {/* Highlight effect for 'Shark Mode' */}
                    {intent.toLowerCase().includes("shark mode") && (
                        <motion.div
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            className="absolute -top-7 left-0 text-[10px] font-pixel text-nobody-accent bg-nobody-accent-soft px-2 py-1 pixel-corners-sm"
                        >
                            SHARK
                        </motion.div>
                    )}
                </div>

                {/* Max spend the AI is allowed to auto-lock into escrow for a match */}
                <input
                    type="number"
                    min="0"
                    step="0.01"
                    value={priceCeiling}
                    onChange={(e) => setPriceCeiling(e.target.value)}
                    placeholder="≤ AVAX"
                    title="Max AVAX the AI may auto-lock into escrow for a matching listing"
                    className="w-20 bg-slate-50 text-nobody-gold p-3 outline-none text-sm placeholder-slate-500 focus:bg-nobody-charcoal transition-colors font-mono text-center"
                />
            </div>
        </div>
    );
};
