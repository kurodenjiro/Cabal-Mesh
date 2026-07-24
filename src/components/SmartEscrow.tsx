import React, { useState, useEffect } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { ContentRecord } from "../types";
import { PixelClassIcon } from "./icons/PixelClassIcon";

interface SmartEscrowProps {
    visible: boolean;
    escrowId: number | null;
    /** "arsenal" deals (from a Marketplace buy()) use releaseDeal/refundDeal, which also
     * move the voucher NFT. "p2p" deals (the mesh DealNotification demo) use the plain
     * Escrow.sol release/refund. */
    dealSource?: "arsenal" | "p2p";
    onClose: () => void;
    onRelease?: () => void;
    itemLabel?: string;
    priceLabel?: string;
    /** Set once the seller's node has delivered the real content over mesh and
     * its signature has been verified against the seller's address. */
    deliveredContent?: ContentRecord;
    /** Real wallet addresses of the two sides, used only to derive a deterministic
     * PixelClassIcon per address — purely cosmetic, no new data fetched here. */
    buyerAddress?: string;
    sellerAddress?: string;
}

export const SmartEscrow: React.FC<SmartEscrowProps> = ({ visible, escrowId, dealSource = "p2p", onClose, onRelease, itemLabel = "NFT #04", priceLabel = "13.5 AVAX", deliveredContent, buyerAddress, sellerAddress }) => {
    const [released, setReleased] = useState(false);
    const [releasing, setReleasing] = useState(false);
    const [refunding, setRefunding] = useState(false);
    const [refunded, setRefunded] = useState(false);
    const [finality, setFinality] = useState<"pending" | "confirmed">("pending");
    // Tauri's webview doesn't reliably surface window.confirm()/alert() as
    // real, clickable dialogs — use in-app state instead so the Refund
    // button never looks unresponsive and errors are actually visible.
    const [confirmingRefund, setConfirmingRefund] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (visible) {
            setReleased(false);
            setReleasing(false);
            setRefunding(false);
            setRefunded(false);
            setFinality("pending");

            if (escrowId == null) return;

            if (dealSource === "arsenal") {
                // buy_listing already awaited the tx receipt before returning the
                // deal id, so it's confirmed on-chain by the time we get here.
                setFinality("confirmed");
            } else {
                invoke("get_escrow_status", { escrowId })
                    .then(() => setFinality("confirmed"))
                    .catch((e) => {
                        console.error("Failed to fetch escrow status:", e);
                        setFinality("pending");
                    });
            }
        }
    }, [visible, escrowId, dealSource]);

    const handleRelease = async () => {
        setError(null);
        if (escrowId == null) {
            setError("No on-chain escrow is associated with this deal.");
            return;
        }

        setReleasing(true);
        try {
            if (dealSource === "arsenal") {
                await invoke("release_deal", { dealId: escrowId });
            } else {
                await invoke("release_escrow", { escrowId });
            }
            setReleased(true);
            onRelease?.();
            setTimeout(() => {
                onClose();
            }, 2000);
        } catch (e) {
            console.error("On-chain release failed:", e);
            setError("On-chain release failed: " + e);
        } finally {
            setReleasing(false);
        }
    };

    const handleRefund = async () => {
        setError(null);
        setConfirmingRefund(false);
        if (escrowId == null) {
            setError("No on-chain escrow is associated with this deal.");
            return;
        }

        setRefunding(true);
        try {
            if (dealSource === "arsenal") {
                await invoke("refund_deal", { dealId: escrowId });
            } else {
                await invoke("refund_escrow", { escrowId });
            }
            setRefunded(true);
            setTimeout(() => {
                onClose();
            }, 2000);
        } catch (e) {
            console.error("On-chain refund failed:", e);
            setError("On-chain refund failed: " + e);
        } finally {
            setRefunding(false);
        }
    };

    if (!visible) return null;

    return (
        <motion.div
            className="absolute inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
        >
            <div className="w-[700px] pixel-corners border border-slate-200 bg-nobody-charcoal shadow-card-lg p-8 relative">
                <button onClick={onClose} className="absolute top-4 right-4 text-slate-400 hover:text-slate-700 transition-colors">✕</button>

                <div className="text-center mb-10">
                    <div className="text-amber-600 text-xs font-semibold tracking-widest mb-2 uppercase">Smart Escrow Protocol</div>
                    <h1 className="text-2xl text-slate-900 font-bold">
                        {released ? "⚔️ Quest Complete!" : refunded ? "Refunded" : "⚔️ Quest In Progress"}
                    </h1>
                </div>

                <div className="flex justify-between items-center mb-12 px-10">
                    {/* Left Vault */}
                    <div className="text-center">
                        <div className="w-20 h-20 rounded-full flex items-center justify-center text-3xl mb-4 bg-amber-50 border border-amber-200">
                            <PixelClassIcon address={buyerAddress} size={32} />
                        </div>
                        <div className="text-slate-900 font-semibold">{priceLabel}</div>
                        <div className="text-xs text-slate-400 mt-1">
                            {released ? "Sent ✓" : refunded ? "Returned to you ✓" : "Payment Locked"}
                        </div>
                    </div>

                    {/* Bridge */}
                    <div className="flex-1 px-4 flex flex-col items-center">
                        <div className="h-[2px] w-full bg-slate-100 relative">
                            <motion.div
                                className="absolute top-1/2 -translate-y-1/2 w-full h-[2px] bg-amber-400"
                                initial={{ scaleX: 0 }}
                                animate={{ scaleX: released || refunded ? 1 : [0, 1] }}
                                transition={{ duration: 2, repeat: released || refunded ? 0 : Infinity }}
                            />
                        </div>
                        <div className="mt-2 text-[11px] text-amber-600 font-medium">
                            {released ? "Complete ✓" : refunded ? "Refunded ✓" : <span className="animate-pulse">Confirming on-chain...</span>}
                        </div>
                    </div>

                    {/* Right Vault */}
                    <div className="text-center">
                        <div className="w-20 h-20 rounded-full flex items-center justify-center text-3xl mb-4 bg-nobody-gold-soft border border-nobody-gold/20">
                            <PixelClassIcon address={sellerAddress} size={32} />
                        </div>
                        <div className="text-slate-900 font-semibold">{itemLabel}</div>
                        <div className="text-xs text-slate-400 mt-1">
                            {released ? "Transferred ✓" : refunded ? "Deal cancelled" : "Item Held Safely"}
                        </div>
                    </div>
                </div>

                {/* Delivered content — real, signature-verified, not a literal ZK proof */}
                {deliveredContent && (
                    <div className="bg-slate-50 pixel-corners-sm p-4 border border-nobody-primary/30 mb-6 space-y-2">
                        <div className="flex justify-between items-center text-xs">
                            <span className="text-nobody-primary font-semibold">📖 Content delivered</span>
                            <span className="text-slate-400 font-mono" title={deliveredContent.signer_address}>
                                verified — signed by {deliveredContent.signer_address.slice(0, 6)}...{deliveredContent.signer_address.slice(-4)}
                            </span>
                        </div>
                        <div className="max-h-40 overflow-y-auto text-xs text-slate-700 whitespace-pre-wrap bg-nobody-charcoal border border-slate-200 pixel-corners-sm p-3">
                            {deliveredContent.text}
                        </div>
                    </div>
                )}

                {/* Status Items */}
                <div className="grid grid-cols-2 gap-4 mb-8">
                    <div className="bg-slate-50 pixel-corners-sm p-3 border border-slate-200 flex justify-between items-center">
                        <span className="text-slate-500 text-xs">Blockchain Confirmed</span>
                        <span className={`text-xs font-semibold ${finality === "confirmed" ? "text-nobody-primary" : "text-slate-400"}`}>
                            {finality === "confirmed" ? "Yes" : "Waiting..."}
                        </span>
                    </div>
                    <div className="bg-slate-50 pixel-corners-sm p-3 border border-slate-200 flex justify-between items-center">
                        <span className="text-slate-500 text-xs">{dealSource === "arsenal" ? "Deal ID" : "Escrow ID"}</span>
                        <span className="text-nobody-primary text-xs font-semibold">{escrowId != null ? `#${escrowId}` : "—"}</span>
                    </div>
                </div>

                {error && (
                    <div className="text-red-600 text-xs text-center mb-3">{error}</div>
                )}

                <div className="space-y-2">
                    <button
                        onClick={handleRelease}
                        disabled={released || releasing || refunding || refunded || escrowId == null}
                        className={`w-full font-semibold py-3 pixel-corners-sm transition-colors shadow-card ${released
                            ? "bg-nobody-primary text-nobody-ink cursor-default"
                            : "bg-amber-500 text-white hover:bg-amber-600 disabled:opacity-50 disabled:cursor-not-allowed"
                            }`}
                    >
                        {released ? "✓ Funds Released" : releasing ? "Releasing..." : "Release Funds"}
                    </button>

                    {!released && confirmingRefund && (
                        <div className="pixel-corners-sm bg-red-50 border border-red-200 p-3 space-y-2">
                            <div className="text-[11px] text-red-700">
                                Refund this escrow back to your wallet? Only do this if you did not receive what was promised.
                            </div>
                            <div className="flex justify-end gap-3 text-[11px]">
                                <button onClick={() => setConfirmingRefund(false)} className="text-slate-400 hover:text-slate-600">Cancel</button>
                                <button onClick={handleRefund} className="text-red-600 font-semibold hover:underline">Yes, refund</button>
                            </div>
                        </div>
                    )}

                    {!released && !confirmingRefund && (
                        <button
                            onClick={() => setConfirmingRefund(true)}
                            disabled={releasing || refunding || refunded || escrowId == null}
                            className={`w-full font-semibold py-2.5 pixel-corners-sm transition-colors text-xs ${refunded
                                ? "bg-slate-100 text-slate-500 cursor-default"
                                : "border border-slate-200 text-slate-500 hover:text-red-600 hover:border-red-300 disabled:opacity-50 disabled:cursor-not-allowed"
                                }`}
                        >
                            {refunded ? "✓ Refunded to your wallet" : refunding ? "Refunding..." : "Refund"}
                        </button>
                    )}
                </div>

            </div>
        </motion.div>
    );
};
