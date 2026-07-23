import React, { useState, useEffect, useCallback } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { VoucherView } from "../types";
import { ItemCard } from "./ItemCard";
import aiComputeIcon from "../assets/items/ai_compute_credit.png";
import relayBandwidthIcon from "../assets/items/relay_bandwidth_credit.png";

// Redeeming one of these two item types applies a permanent boost to this
// node's local relay-earnings estimate (see useRelayStats.ts) instead of
// just unlocking a generic "service" — same honesty level as the rest of
// the relay-reward estimate (no real fund movement, just a local multiplier).
const RELAY_BOOST_ITEMS: Record<string, { icon: string; boost: number }> = {
    "AI Compute Credit": { icon: aiComputeIcon, boost: 0.25 },
    "Relay Bandwidth Credit": { icon: relayBandwidthIcon, boost: 0.5 },
};

interface RedeemVoucherProps {
    visible: boolean;
    /** Bumped by App.tsx whenever ownership may have changed elsewhere (e.g.
     * releasing escrow funds, which is when the voucher NFT actually
     * transfers to the buyer) — triggers a re-fetch while this is open. */
    refreshKey?: number;
    onClose: () => void;
    /** Called after successfully listing an owned item for resale, so the
     * Trading Post panel refreshes and picks it up. */
    onListed?: () => void;
}

/** The third page: the buyer proves on-chain they hold a voucher, then burns it to claim the service. */
export const RedeemVoucher: React.FC<RedeemVoucherProps> = ({ visible, refreshKey, onClose, onListed }) => {
    const [myAddress, setMyAddress] = useState<string | null>(null);
    const [vouchers, setVouchers] = useState<VoucherView[]>([]);
    const [loading, setLoading] = useState(false);
    const [selected, setSelected] = useState<VoucherView | null>(null);
    const [ownerCheck, setOwnerCheck] = useState<"checking" | "verified" | "failed" | null>(null);
    const [redeeming, setRedeeming] = useState(false);
    const [redeemed, setRedeemed] = useState<string | null>(null);
    const [boostApplied, setBoostApplied] = useState<number | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [resellPrice, setResellPrice] = useState("");
    const [reselling, setReselling] = useState(false);
    const [relisted, setRelisted] = useState(false);

    const load = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            const identities = await invoke<{ address: string }[]>("get_identity");
            const address = identities[0]?.address;
            setMyAddress(address ?? null);
            if (address) {
                const owned = await invoke<VoucherView[]>("get_owned_vouchers", { owner: address });
                // Exclude vouchers you minted yourself to sell and still hold unsold —
                // there's nothing to "claim" there, you're just the seller sitting on stock.
                // Only vouchers actually acquired from someone else belong here.
                setVouchers(owned.filter((v) => v.minted_by.toLowerCase() !== address.toLowerCase()));
            }
        } catch (e) {
            console.error("Failed to load owned vouchers:", e);
            setError("Failed to load your vouchers: " + e);
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        if (visible) {
            setSelected(null);
            setOwnerCheck(null);
            setRedeemed(null);
            setBoostApplied(null);
            setRelisted(false);
            load();
        }
    }, [visible, load]);

    // Ownership can change elsewhere (e.g. releasing escrow funds is when the
    // voucher NFT actually transfers to the buyer) — re-fetch without
    // resetting the user's current selection.
    useEffect(() => {
        if (visible) load();
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [refreshKey]);

    const handleSelect = async (v: VoucherView) => {
        setSelected(v);
        setRedeemed(null);
        setRelisted(false);
        setResellPrice("");
        setOwnerCheck("checking");
        try {
            const owner = await invoke<string>("get_voucher_owner", { tokenId: v.token_id });
            setOwnerCheck(myAddress && owner.toLowerCase() === myAddress.toLowerCase() ? "verified" : "failed");
        } catch (e) {
            console.error("Ownership check failed:", e);
            setOwnerCheck("failed");
        }
    };

    const handleRedeem = async () => {
        if (!selected || ownerCheck !== "verified") return;
        setRedeeming(true);
        setError(null);
        setBoostApplied(null);
        try {
            await invoke("redeem_voucher", { tokenId: selected.token_id });

            const boostItem = RELAY_BOOST_ITEMS[selected.voucher_type];
            if (boostItem) {
                try {
                    await invoke("apply_relay_boost", { additional: boostItem.boost });
                    setBoostApplied(boostItem.boost);
                } catch (e) {
                    console.error("Failed to apply relay boost:", e);
                }
            }

            setRedeemed(selected.voucher_type);
            setVouchers((prev) => prev.filter((v) => v.token_id !== selected.token_id));
            setSelected(null);
            setOwnerCheck(null);
        } catch (e) {
            console.error("Redeem failed:", e);
            setError("On-chain redeem failed: " + e);
        } finally {
            setRedeeming(false);
        }
    };

    const handleResell = async () => {
        const price = parseFloat(resellPrice);
        if (!selected || ownerCheck !== "verified" || !(price > 0)) return;
        setReselling(true);
        setError(null);
        try {
            await invoke("approve_voucher", { tokenId: selected.token_id });
            await invoke<number>("create_asset_listing", {
                description: selected.description,
                priceAvax: resellPrice,
                tokenId: selected.token_id,
            });
            setRelisted(true);
            setVouchers((prev) => prev.filter((v) => v.token_id !== selected.token_id));
            setSelected(null);
            setOwnerCheck(null);
            onListed?.();
        } catch (e) {
            console.error("Resell failed:", e);
            setError("On-chain resell listing failed: " + e);
        } finally {
            setReselling(false);
        }
    };

    if (!visible) return null;

    return (
        <motion.div
            className="absolute inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm text-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
        >
            <div className="w-[650px] max-h-[85vh] flex flex-col hud-frame border border-nobody-gold/30 bg-nobody-charcoal shadow-card-lg overflow-hidden">

                {/* Header */}
                <div className="bg-slate-50 px-5 py-3 border-b border-nobody-gold/20 flex justify-between items-center text-xs">
                    <span className="text-nobody-gold font-pixel text-[10px] tracking-wide">🎒 INVENTORY</span>
                    <button onClick={onClose} className="text-slate-400 hover:text-slate-700 transition-colors">✕</button>
                </div>

                <div className="p-6 space-y-6 overflow-y-auto">

                    {redeemed && (
                        <div className="bg-nobody-primary-soft/40 border border-nobody-primary/30 pixel-corners-sm p-4 text-center">
                            <div className="text-nobody-primary font-semibold">✓ Claimed: {redeemed}</div>
                            {boostApplied != null ? (
                                <div className="text-nobody-gold text-xs mt-1 font-semibold">
                                    ⚡ +{boostApplied.toFixed(2)}x relay-earnings boost applied (permanent, this device)
                                </div>
                            ) : (
                                <div className="text-slate-500 text-xs mt-1">Item burned on-chain. Service unlocked.</div>
                            )}
                        </div>
                    )}

                    {error && <div className="text-red-600 text-xs">{error}</div>}

                    <div>
                        <div className="text-slate-400 text-[10px] font-pixel tracking-wide mb-2">
                            ITEMS {myAddress && <span className="font-mono normal-case">({myAddress.slice(0, 6)}...{myAddress.slice(-4)})</span>}
                        </div>

                        {loading && <div className="text-slate-400 text-xs italic">Checking on-chain ownership...</div>}
                        {!loading && vouchers.length === 0 && (
                            <div className="text-slate-400 text-xs italic">No items found in your wallet. Buy one from a matched Trading Post listing first.</div>
                        )}

                        <div className="space-y-2">
                            {vouchers.map((v) => (
                                <ItemCard
                                    key={v.token_id}
                                    icon={
                                        RELAY_BOOST_ITEMS[v.voucher_type]
                                            ? <img src={RELAY_BOOST_ITEMS[v.voucher_type].icon} alt="" draggable={false} style={{ width: 20, height: 20, imageRendering: "pixelated" }} />
                                            : "🎫"
                                    }
                                    title={`#${v.token_id} ${v.voucher_type}`}
                                    subtitle={v.description}
                                    priceAvax={0}
                                    priceLabel="✓ Owned"
                                    showRarity={false}
                                    onClick={() => handleSelect(v)}
                                    selected={selected?.token_id === v.token_id}
                                />
                            ))}
                        </div>
                    </div>

                    {selected && (
                        <div className="bg-slate-50 pixel-corners-sm p-4 border border-slate-200 space-y-3">
                            <div className="flex justify-between items-center text-xs">
                                <span className="text-slate-500">On-chain ownership check</span>
                                {ownerCheck === "checking" && <span className="text-slate-400 animate-pulse">Verifying...</span>}
                                {ownerCheck === "verified" && <span className="text-nobody-primary font-semibold">✓ Confirmed — you own this</span>}
                                {ownerCheck === "failed" && <span className="text-red-500 font-semibold">✗ Not confirmed</span>}
                            </div>

                            <button
                                onClick={handleRedeem}
                                disabled={ownerCheck !== "verified" || redeeming || reselling}
                                className="w-full bg-nobody-gold text-white font-semibold py-3 pixel-corners-sm hover:brightness-110 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                            >
                                {redeeming ? "Looting..." : "🎁 Loot This Item"}
                            </button>

                            <div className="border-t border-slate-200 pt-3 space-y-2">
                                <div className="text-slate-500 text-[11px]">Or sell it back on the Trading Post:</div>
                                <div className="flex gap-2">
                                    <input
                                        type="number"
                                        min="0"
                                        step="0.001"
                                        value={resellPrice}
                                        onChange={(e) => setResellPrice(e.target.value)}
                                        disabled={ownerCheck !== "verified" || reselling || redeeming}
                                        placeholder="Price (AVAX)"
                                        className="flex-1 bg-white border border-slate-200 pixel-corners-sm p-2 text-xs text-slate-900 outline-none focus:border-nobody-gold disabled:opacity-50"
                                    />
                                    <button
                                        onClick={handleResell}
                                        disabled={ownerCheck !== "verified" || reselling || redeeming || !(parseFloat(resellPrice) > 0)}
                                        className="text-xs font-semibold px-4 pixel-corners-sm border border-nobody-gold text-nobody-gold hover:bg-nobody-gold-soft transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                                    >
                                        {reselling ? "Listing..." : "🔁 Resell"}
                                    </button>
                                </div>
                            </div>
                        </div>
                    )}

                    {relisted && (
                        <div className="bg-nobody-gold-soft/40 border border-nobody-gold/30 pixel-corners-sm p-4 text-center">
                            <div className="text-nobody-gold font-semibold">🔁 Listed on the Trading Post</div>
                            <div className="text-slate-500 text-xs mt-1">Anyone else on the mesh can now buy it.</div>
                        </div>
                    )}
                </div>

                {/* Footer */}
                <div className="bg-slate-50 border-t border-slate-200 px-5 py-2 flex justify-end items-center text-xs">
                    <button
                        onClick={onClose}
                        className="bg-nobody-charcoal text-slate-500 font-pixel text-[10px] px-6 py-2 hover:text-nobody-gold transition-colors pixel-corners-sm border border-slate-300 hover:border-nobody-gold shadow-card"
                    >
                        [ BACK ]
                    </button>
                </div>
            </div>
        </motion.div>
    );
};
