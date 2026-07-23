import React, { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DealView } from "../types";

interface QuestLogPanelProps {
    refreshKey?: number;
    /** Opens the Smart Escrow screen for a deal — lets the buyer come back and
     * release funds for a deal that didn't route there automatically (e.g. one
     * confirmed later via the offline-relay retry, not the live buy flow). */
    onOpenDeal?: (deal: DealView) => void;
}

function statusIcon(status: DealView["status"]) {
    if (status === "active") return "⚔️";
    if (status === "released") return "✅";
    if (status === "refunded") return "↩️";
    return "•";
}

/** Always-visible panel on the Nexus screen showing real on-chain deals —
 * no separate "Arsenal Mode" page needed just to see what's in progress. */
export const QuestLogPanel: React.FC<QuestLogPanelProps> = ({ refreshKey, onOpenDeal }) => {
    const [deals, setDeals] = useState<DealView[]>([]);

    const load = useCallback(async () => {
        try {
            const identities = await invoke<{ address: string }[]>("get_identity");
            const address = identities[0]?.address;
            if (address) {
                const myDeals = await invoke<DealView[]>("get_my_deals", { address });
                setDeals(myDeals);
            }
        } catch (e) {
            console.error("Failed to load deals:", e);
        }
    }, []);

    useEffect(() => {
        load();
        const interval = setInterval(load, 10000);
        return () => clearInterval(interval);
    }, [load, refreshKey]);

    return (
        <div className="absolute top-28 right-6 w-72 z-20 hud-frame border border-slate-200 bg-nobody-charcoal shadow-card p-3">
            <div className="w-full text-left mb-2">
                <span className="text-nobody-primary font-pixel text-[10px] tracking-wide">📜 QUEST LOG</span>
            </div>
            {deals.length === 0 ? (
                <div className="text-slate-400 text-xs italic">No deals yet — matched buys/sells show up here.</div>
            ) : (
                <div className="space-y-1.5 max-h-56 overflow-y-auto">
                    {deals.slice(-5).reverse().map((d) => {
                        const counterparty = d.role === "seller" ? d.buyer : d.seller;
                        const canRelease = d.role === "buyer" && d.status === "active" && onOpenDeal;
                        const Wrapper = canRelease ? "button" : "div";
                        return (
                            <Wrapper
                                key={d.deal_id}
                                onClick={canRelease ? () => onOpenDeal(d) : undefined}
                                className={`w-full text-left bg-slate-50 pixel-corners-sm border border-slate-200 p-2 text-xs space-y-0.5 ${canRelease ? "hover:bg-slate-100 transition-colors cursor-pointer" : ""}`}
                            >
                                <div className="flex items-center justify-between gap-2">
                                    <span className="text-slate-900 truncate">
                                        {statusIcon(d.status)} {d.role === "seller" ? "💰 Sold" : "🛒 Bought"} 🎫 #{d.token_id}
                                    </span>
                                    <span className="text-nobody-primary font-semibold shrink-0">{(parseFloat(d.amount_avax) || 0).toFixed(2)} AVAX</span>
                                </div>
                                <div className="text-slate-400 text-[10px] font-mono truncate">
                                    {d.role === "seller" ? "Buyer" : "Seller"}: {counterparty.slice(0, 8)}...{counterparty.slice(-6)}
                                </div>
                                {canRelease && (
                                    <div className="text-nobody-gold text-[10px] pt-0.5">→ Tap to release funds / claim item</div>
                                )}
                            </Wrapper>
                        );
                    })}
                </div>
            )}
        </div>
    );
};

export default QuestLogPanel;
