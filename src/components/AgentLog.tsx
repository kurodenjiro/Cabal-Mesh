import React from "react";
import { motion, AnimatePresence } from "framer-motion";

interface AgentLogProps {
    visible: boolean;
    logs: string[];
}

export const AgentLog: React.FC<AgentLogProps> = ({ visible, logs }) => {
    if (!visible) return null;

    return (
        <AnimatePresence>
            <motion.div
                className="absolute inset-x-4 top-[15%] mx-auto w-[700px] z-50 text-sm"
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -20 }}
            >
                <div className="bg-nobody-charcoal border border-slate-200 shadow-card-lg overflow-hidden rounded-2xl">
                    {/* Header */}
                    <div className="bg-slate-50 border-b border-slate-200 p-3 text-center">
                        <span className="text-slate-900 font-semibold tracking-wide text-xs uppercase">Local Agent Processing</span>
                    </div>

                    <div className="p-6 space-y-4">
                        {/* Processing Steps */}
                        {logs.map((log, i) => (
                            <motion.div
                                key={i}
                                initial={{ opacity: 0, x: -10 }}
                                animate={{ opacity: 1, x: 0 }}
                                className="space-y-1"
                            >
                                <div className="text-slate-600 text-xs font-mono">
                                    {log}
                                </div>
                                {i === logs.length - 1 && (
                                    <div className="mt-2 bg-slate-100 rounded-full h-1.5 overflow-hidden">
                                        <motion.div
                                            className="h-full bg-nobody-mint rounded-full"
                                            initial={{ width: "0%" }}
                                            animate={{ width: "100%" }}
                                            transition={{ duration: 1.5 }}
                                        />
                                    </div>
                                )}
                            </motion.div>
                        ))}
                    </div>
                </div>
            </motion.div>
        </AnimatePresence>
    );
};
