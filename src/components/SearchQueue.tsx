import React from "react";
import { motion, AnimatePresence } from "framer-motion";

export interface SearchJob {
    id: number;
    intent: string;
    status: "scanning" | "buying" | "matched" | "no-match" | "error" | "queued";
    message: string;
}

interface SearchQueueProps {
    jobs: SearchJob[];
    onDismiss: (id: number) => void;
}

/** A compact, stacked queue of in-flight/finished intent searches — lets the
 * user fire off several intents without waiting for each one to resolve,
 * and dismiss any that are done (or still just scanning — no funds are at
 * risk until a job reaches "buying"). */
export const SearchQueue: React.FC<SearchQueueProps> = ({ jobs, onDismiss }) => {
    if (jobs.length === 0) return null;

    return (
        <div className="absolute bottom-28 left-6 w-[440px] z-20 flex flex-col-reverse gap-2 max-h-64 overflow-y-auto">
            <AnimatePresence>
                {jobs.map((job) => (
                    <motion.div
                        key={job.id}
                        initial={{ opacity: 0, y: 8 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -8 }}
                        className={`bg-nobody-charcoal/80 border shadow-card pixel-corners-sm px-3 py-2 flex items-center gap-3 text-xs opacity-85 ${job.status === "matched" ? "border-nobody-primary" :
                            job.status === "no-match" ? "border-slate-300" :
                                job.status === "error" ? "border-red-300" :
                                    job.status === "queued" ? "border-nobody-gold" :
                                        job.status === "buying" ? "border-nobody-primary" : "border-nobody-gold/40"
                            }`}
                    >
                        {(job.status === "scanning" || job.status === "buying") && (
                            <div className="w-3.5 h-3.5 border-2 border-nobody-gold border-t-transparent rounded-full animate-spin shrink-0" />
                        )}
                        {job.status === "matched" && <span className="shrink-0">✅</span>}
                        {job.status === "no-match" && <span className="shrink-0">➖</span>}
                        {job.status === "error" && <span className="shrink-0">⚠️</span>}
                        {job.status === "queued" && <span className="shrink-0">📡</span>}

                        <div className="flex-1 min-w-0">
                            <div className="text-slate-700 font-medium truncate">🗺️ Quest: "{job.intent}"</div>
                            <div className="text-slate-400 text-[11px] truncate">{job.message}</div>
                        </div>

                        {job.status !== "buying" && job.status !== "matched" && (
                            <button
                                onClick={() => onDismiss(job.id)}
                                className="text-slate-400 hover:text-red-500 transition-colors shrink-0"
                                title={job.status === "scanning" ? "Dismiss (no funds involved yet)" : "Dismiss"}
                            >
                                ✕
                            </button>
                        )}
                    </motion.div>
                ))}
            </AnimatePresence>
        </div>
    );
};
