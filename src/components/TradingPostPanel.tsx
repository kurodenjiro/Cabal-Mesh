import React, { useState, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { AssetListingView } from "../types";
import { ItemCard } from "./ItemCard";

interface TradingPostPanelProps {
    refreshKey?: number;
    onCreateListing: () => void;
}

/** Always-visible panel on the Nexus screen showing real on-chain listings —
 * no separate "Arsenal Mode" page needed just to see what's for sale. */
export const TradingPostPanel: React.FC<TradingPostPanelProps> = ({ refreshKey, onCreateListing }) => {
    const [listings, setListings] = useState<AssetListingView[]>([]);
    const [myAddress, setMyAddress] = useState<string | null>(null);
    const [selectedId, setSelectedId] = useState<string>("");

    const load = useCallback(() => {
        invoke<AssetListingView[]>("get_active_asset_listings")
            .then(setListings)
            .catch((e) => console.error("Failed to load listings:", e));
    }, []);

    useEffect(() => {
        load();
        const interval = setInterval(load, 10000);
        return () => clearInterval(interval);
    }, [load, refreshKey]);

    useEffect(() => {
        // Backend identity-loading is async and can still be in progress at mount —
        // a one-shot fetch can race it and come back empty forever, so poll until ready.
        let cancelled = false;
        const tryFetch = () => {
            invoke<{ address: string }[]>("get_identity")
                .then((ids) => {
                    if (cancelled) return;
                    const address = ids[0]?.address;
                    if (address) setMyAddress(address);
                    else setTimeout(tryFetch, 1000);
                })
                .catch(() => { if (!cancelled) setTimeout(tryFetch, 1000); });
        };
        tryFetch();
        return () => { cancelled = true; };
    }, []);

    // You can't buy your own listing (the Marketplace rejects it), so a "shop
    // window" of things to buy shouldn't include items you're selling yourself.
    const buyableListings = myAddress
        ? listings.filter((l) => l.seller.toLowerCase() !== myAddress.toLowerCase())
        : listings;

    const selectedListing = buyableListings.find((l) => String(l.id) === selectedId) ?? null;

    return (
        <div className="absolute top-28 left-6 w-72 z-20 hud-frame border border-slate-200 bg-nobody-charcoal shadow-card p-3">
            <div className="w-full text-left mb-2">
                <span className="text-nobody-gold font-pixel text-[10px] tracking-wide">🗡️ TRADING POST ({buyableListings.length})</span>
                <div className="text-slate-400 text-[10px] mt-0.5">Items other sellers have listed — not yours</div>
            </div>
            {buyableListings.length === 0 ? (
                <div className="text-slate-400 text-xs italic">No active listings yet.</div>
            ) : (
                <div className="space-y-1.5 max-h-56 overflow-y-auto">
                    {buyableListings.slice(0, 5).map((l) => (
                        <ItemCard
                            key={l.id}
                            icon="🎫"
                            title={`#${l.token_id} ${l.description}`}
                            subtitle={`Seller: ${l.seller.slice(0, 6)}...${l.seller.slice(-4)}`}
                            priceAvax={parseFloat(l.price_avax) || 0}
                            onClick={() => setSelectedId((cur) => (cur === String(l.id) ? "" : String(l.id)))}
                            selected={selectedId === String(l.id)}
                        />
                    ))}
                </div>
            )}

            {selectedListing && createPortal(
                <div
                    className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 backdrop-blur-sm"
                    onClick={() => setSelectedId("")}
                >
                    <div
                        className="w-[380px] bg-nobody-charcoal hud-frame border border-nobody-gold/30 shadow-card-lg p-4 text-xs space-y-2"
                        onClick={(e) => e.stopPropagation()}
                    >
                        <div className="flex justify-between items-center mb-2">
                            <span className="text-nobody-gold font-pixel text-[10px] tracking-wide">🎫 ITEM DETAIL</span>
                            <button onClick={() => setSelectedId("")} className="text-slate-400 hover:text-slate-700 transition-colors">✕</button>
                        </div>
                        <div className="flex justify-between"><span className="text-slate-500">Seller</span><span className="text-slate-900 font-mono">{selectedListing.seller.slice(0, 8)}...{selectedListing.seller.slice(-6)}</span></div>
                        <div className="flex justify-between"><span className="text-slate-500">Item</span><span className="text-slate-900">🎫 #{selectedListing.token_id}</span></div>
                        <div className="flex justify-between"><span className="text-slate-500">Price</span><span className="text-nobody-gold font-semibold">{(parseFloat(selectedListing.price_avax) || 0).toFixed(2)} AVAX</span></div>
                        <div className="text-slate-500 pt-1 border-t border-slate-200 mt-1">Description</div>
                        <div className="text-slate-700 whitespace-pre-wrap break-words max-h-40 overflow-y-auto">{selectedListing.description}</div>
                        <div className="text-slate-400 text-[10px] pt-1 border-t border-slate-200 mt-1">
                            To buy this, type a matching intent below (e.g. reference this description).
                        </div>
                    </div>
                </div>,
                document.body
            )}

            <button
                onClick={onCreateListing}
                className="w-full mt-2 text-left text-xs text-slate-400 hover:text-slate-900 border border-dashed border-slate-200 hover:border-slate-300 pixel-corners-sm p-2 transition-colors"
            >
                + Create New Listing
            </button>
        </div>
    );
};

export default TradingPostPanel;
