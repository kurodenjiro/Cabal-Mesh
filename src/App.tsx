import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { formatEther } from "ethers";
import "./styles.css";
import skullIcon from "./assets/icons/icon_skull.png";

// Components
import { Nexus } from "./components/Nexus";
import { MeshCharacterField } from "./components/MeshCharacterField";
import { AmbientParticles } from "./components/AmbientParticles";
import { RelayerStatusCompact } from "./components/RelayerStatusCompact";
import { WalletBalanceCompact } from "./components/WalletBalanceCompact";
import { IntentComposer } from "./components/IntentComposer";
import { SearchQueue, SearchJob } from "./components/SearchQueue";
import { OfflineQueue } from "./components/OfflineQueue";
import { OnboardingHint } from "./components/OnboardingHint";
import { RATE_PER_BYTE_AVAX } from "./hooks/useRelayStats";
import { useOllamaStatus } from "./hooks/useOllamaStatus";
import { NotificationToast } from "./components/NotificationToast";
import { TradingPostPanel } from "./components/TradingPostPanel";
import { QuestLogPanel } from "./components/QuestLogPanel";
import { ServiceCreator } from "./components/ServiceCreator";
import { WalletCabinet } from "./components/WalletCabinet";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { SmartEscrow } from "./components/SmartEscrow";
import { RedeemVoucher } from "./components/RedeemVoucher";
import { DelegationCenter } from "./components/DelegationCenter";

// Types
import { Peer, MeshEvent, ViewState, MatchResult, TxResult, QueuedTx, ContentRecord, DealView } from "./types";

function App() {
    const [intent, setIntent] = useState("");
    const [peers, setPeers] = useState<Peer[]>([]);
    const [isOnline, setIsOnline] = useState(navigator.onLine);
    const aiReady = useOllamaStatus();
    const [showHint, setShowHint] = useState(() => localStorage.getItem("cabalmesh_seen_intent_hint") !== "1");
    const dismissHint = () => {
        localStorage.setItem("cabalmesh_seen_intent_hint", "1");
        setShowHint(false);
    };

    // UI State
    const [view, setView] = useState<ViewState>("nexus");
    const [notification, setNotification] = useState<string | null>(null);
    // Tracks whether this node currently has any of its own intents in flight, so the
    // mesh-event listener (registered once) can ignore its own broadcast echoing back.
    // A ref (not state) because the listener's closure would otherwise see a stale value.
    const isSenderRef = useRef(false);
    const activeSearchCountRef = useRef(0);
    const dismissedSearchesRef = useRef<Set<number>>(new Set());
    const nextSearchIdRef = useRef(0);
    const [searches, setSearches] = useState<SearchJob[]>([]);
    const [escrowId, setEscrowId] = useState<number | null>(null);
    const [isRelaying, setIsRelaying] = useState(false);
    const [priceCeiling, setPriceCeiling] = useState("1");
    const [dealItemLabel, setDealItemLabel] = useState("NFT #04");
    const [dealPriceLabel, setDealPriceLabel] = useState("13.5 AVAX");
    const [listingsRefreshKey, setListingsRefreshKey] = useState(0);
    const [dealSource, setDealSource] = useState<"arsenal" | "p2p">("p2p");

    // Content-delivery: which voucher tokenId + seller the currently open deal is
    // for, and any content that's arrived (verified) for it. Refs mirror the state
    // so the mesh-event listener (registered once) never sees a stale value.
    const [dealTokenId, setDealTokenId] = useState<number | null>(null);
    const dealTokenIdRef = useRef<number | null>(null);
    const dealSellerRef = useRef<string | null>(null);
    const [deliveredContent, setDeliveredContent] = useState<Record<number, ContentRecord>>({});

    // Own real wallet address — used for cosmetic class-icon framing in
    // SmartEscrow (which "class" avatar represents "you" in a deal) and for
    // announcing our real identity to mesh peers (see the presence broadcast
    // below). Backend bootstrap (loading identities) runs async and can still
    // be in progress when this component first mounts, so a single one-shot
    // fetch can race it and come back empty forever — poll until it succeeds.
    const [myAddress, setMyAddress] = useState<string | null>(null);
    useEffect(() => {
        let cancelled = false;
        const tryFetch = () => {
            invoke<{ address: string }[]>("get_identity")
                .then((ids) => {
                    if (cancelled) return;
                    const address = ids[0]?.address;
                    if (address) setMyAddress(address);
                    else setTimeout(tryFetch, 1000);
                })
                .catch((e) => {
                    console.error("Failed to fetch identity, retrying:", e);
                    if (!cancelled) setTimeout(tryFetch, 1000);
                });
        };
        tryFetch();
        return () => { cancelled = true; };
    }, []);

    // Announce our real wallet address over the mesh whenever we have one and
    // whenever a new peer joins — mDNS discovery alone only reveals ephemeral
    // libp2p PeerIDs, not who's actually behind them.
    useEffect(() => {
        if (!myAddress) return;
        invoke("send_intent_to_mesh", {
            payload: JSON.stringify({ type: "Presence", address: myAddress }),
        }).catch(console.error);
    }, [myAddress, peers.length]);

    // Real, periodically-refreshed AVAX balance — used to reject an intent whose
    // max price already exceeds what the wallet actually holds, instead of letting
    // it scan/match and only fail once it tries to actually buy.
    const [myBalanceAvax, setMyBalanceAvax] = useState<number | null>(null);
    useEffect(() => {
        const refreshBalance = async () => {
            try {
                await invoke("sync_blockchain_state", { wallet: "" });
                const snapshot = await invoke<{ assets: { symbol: string; amount: string }[] } | null>("get_wallet_snapshot");
                const native = snapshot?.assets?.find((a) => a.symbol === "AVAX");
                setMyBalanceAvax(native ? parseFloat(formatEther(native.amount)) : 0);
            } catch (e) {
                console.error("Failed to refresh balance:", e);
            }
        };
        refreshBalance();
        const interval = setInterval(refreshBalance, 15000);
        return () => clearInterval(interval);
    }, []);

    // Offline-mesh-relay: transactions signed locally (no RPC reachable) and queued
    // for whichever peer with real connectivity + Relay Mode on submits them.
    const [offlineQueue, setOfflineQueue] = useState<QueuedTx[]>([]);
    // A ref (not state) so the mesh-event listener's closure (registered once) always
    // sees the current Relay Mode toggle, instead of the value at mount time.
    const isRelayingRef = useRef(false);
    useEffect(() => { isRelayingRef.current = isRelaying; }, [isRelaying]);

    useEffect(() => {
        invoke<QueuedTx[]>("get_pending_relay_txs").then(setOfflineQueue).catch(console.error);
    }, []);

    // A queued tx originally failed because the RPC timed out for ~6s — that's
    // usually transient, not a real extended offline period. Previously the
    // *only* way a queued tx ever got submitted was another peer relaying it
    // (fire-and-forget over mesh, easy to miss if no one's listening at that
    // exact moment). Retry our own queued txs directly too, since we likely
    // have real connectivity again seconds later.
    const offlineQueueRef = useRef<QueuedTx[]>([]);
    useEffect(() => { offlineQueueRef.current = offlineQueue; }, [offlineQueue]);

    useEffect(() => {
        const retryQueued = async () => {
            for (const item of offlineQueueRef.current) {
                if (item.status !== "queued") continue;
                try {
                    const txHash = await invoke<string>("submit_raw_transaction", { rawTxHex: item.raw_tx_hex });
                    console.log("✅ Self-submitted previously-queued tx:", item.id, txHash);
                    await invoke("mark_relay_tx_status", { queueId: item.id, status: "confirmed", txHash });
                    setOfflineQueue((prev) => prev.map((t) => (t.id === item.id ? { ...t, status: "confirmed", tx_hash: txHash } : t)));
                    setListingsRefreshKey((k) => k + 1);
                } catch (e) {
                    console.log("Still can't submit queued tx (will retry):", item.id, e);
                }
            }
        };
        const interval = setInterval(retryQueued, 8000);
        return () => clearInterval(interval);
    }, []);

    const handleDismissOfflineItem = (id: string) => {
        setOfflineQueue((prev) => prev.filter((t) => t.id !== id));
    };

    useEffect(() => {
        const unlisten = listen<MeshEvent>("mesh-event", async (event) => {
            console.log("🔔 Mesh event received:", event.payload);
            const meshEvent = event.payload;

            if (meshEvent.type === "PeerDiscovered") {
                console.log("✅ PeerDiscovered event");
                setPeers((prev) => [...prev, {
                    id: meshEvent.peer_id || "",
                    address: meshEvent.address || "",
                    timestamp: Date.now(),
                }]);
            } else if (meshEvent.type === "PeerIdentity") {
                // A peer announced its real AVAX wallet address over the mesh.
                const peerId = meshEvent.peer_id;
                const walletAddress = meshEvent.address;
                if (peerId && walletAddress) {
                    setPeers((prev) => prev.map((p) => (p.id === peerId ? { ...p, walletAddress } : p)));
                }
            } else if (meshEvent.type === "SettlementComplete") {
                console.log("✅ Settlement confirmation received!", meshEvent);
                setNotification("✅ Funds released! Transaction complete.");

            } else if (meshEvent.type === "RelayTxReceived") {
                const queueId = meshEvent.queue_id || "";
                const rawTxHex = meshEvent.raw_tx_hex || "";
                const summary = meshEvent.summary || "";
                console.log("📡 RelayTxReceived:", queueId, summary, "Relay Mode on?", isRelayingRef.current);

                if (!isRelayingRef.current || !rawTxHex) {
                    // Relay Mode is off on this node — leave it for another peer to pick up.
                    return;
                }

                try {
                    const txHash = await invoke<string>("submit_raw_transaction", { rawTxHex });
                    console.log("✅ Relayed transaction for peer:", queueId, txHash);
                    invoke("send_intent_to_mesh", {
                        payload: JSON.stringify({ type: "RelayConfirmed", queue_id: queueId, status: "confirmed", tx_hash: txHash }),
                    }).catch(console.error);

                    // Real, persisted credit for helping — reward is a deterministic estimate
                    // (bytes relayed × the same rate Relay Mode's own stats use), not an actual payout.
                    const rewardAvax = ((rawTxHex.replace(/^0x/, "").length / 2) * RATE_PER_BYTE_AVAX).toFixed(6);
                    invoke("record_relayed_tx", { summary, txHash, rewardAvax }).catch(console.error);
                } catch (e) {
                    console.error("Failed to relay transaction:", e);
                    invoke("send_intent_to_mesh", {
                        payload: JSON.stringify({ type: "RelayConfirmed", queue_id: queueId, status: "failed" }),
                    }).catch(console.error);
                }

            } else if (meshEvent.type === "RelayConfirmed") {
                const queueId = meshEvent.queue_id || "";
                const status = meshEvent.status || "failed";
                const txHash = meshEvent.tx_hash;
                console.log("📨 RelayConfirmed:", queueId, status, txHash);

                invoke("mark_relay_tx_status", { queueId, status, txHash: txHash ?? null }).catch(console.error);
                setOfflineQueue((prev) => prev.map((t) => (t.id === queueId ? { ...t, status: status as QueuedTx["status"], tx_hash: txHash ?? null } : t)));

            } else if (meshEvent.type === "ContentRequested") {
                const tokenId = meshEvent.token_id;
                if (tokenId == null) return;
                console.log("📨 ContentRequested for token", tokenId);
                try {
                    const content = await invoke<ContentRecord | null>("get_content", { tokenId });
                    if (content) {
                        invoke("send_intent_to_mesh", {
                            payload: JSON.stringify({
                                type: "ContentDelivery",
                                token_id: tokenId,
                                text: content.text,
                                signature: content.signature,
                                signer_address: content.signer_address,
                            }),
                        }).catch(console.error);
                    }
                } catch (e) {
                    console.error("Failed to look up content for delivery:", e);
                }

            } else if (meshEvent.type === "ContentDelivered") {
                const tokenId = meshEvent.token_id;
                const text = meshEvent.text || "";
                const signature = meshEvent.signature || "";
                const signerAddress = meshEvent.signer_address || "";
                if (tokenId == null || !text || tokenId !== dealTokenIdRef.current || !dealSellerRef.current) return;

                console.log("📬 ContentDelivered for token", tokenId, "verifying against seller", dealSellerRef.current);
                try {
                    const verified = await invoke<boolean>("receive_content", {
                        tokenId,
                        text,
                        signature,
                        expectedSeller: dealSellerRef.current,
                    });
                    if (verified) {
                        setDeliveredContent((prev) => ({
                            ...prev,
                            [tokenId]: { token_id: tokenId, text, fingerprint: "", signature, signer_address: signerAddress },
                        }));
                    } else {
                        console.error("Content delivery failed signature verification — rejected.");
                    }
                } catch (e) {
                    console.error("Failed to verify delivered content:", e);
                }

            } else {
                console.log("⚠️ Unknown mesh event type:", meshEvent.type);
            }
        });

        const handleOnline = () => setIsOnline(true);
        const handleOffline = () => setIsOnline(false);
        window.addEventListener("online", handleOnline);
        window.addEventListener("offline", handleOffline);

        return () => {
            unlisten.then((fn) => fn());
            window.removeEventListener("online", handleOnline);
            window.removeEventListener("offline", handleOffline);
        };
    }, []);

    const updateSearch = (id: number, patch: Partial<SearchJob>) => {
        setSearches(prev => prev.map(j => (j.id === id ? { ...j, ...patch } : j)));
    };

    /** Removes a search from view. For a still-scanning job this is a soft,
     * UI-only cancel — no funds are at risk yet (match_intent_to_listings
     * doesn't spend anything), so we just ignore its result when it resolves. */
    const handleDismissSearch = (id: number) => {
        dismissedSearchesRef.current.add(id);
        setSearches(prev => prev.filter(j => j.id !== id));
    };

    /** Fires one intent search as an independent, cancellable job — multiple
     * of these can be in flight at once, tracked in `searches`. */
    const runSearch = async (id: number, submittedIntent: string, ceiling: number) => {
        activeSearchCountRef.current += 1;
        isSenderRef.current = true;

        const finishSearch = () => {
            activeSearchCountRef.current = Math.max(0, activeSearchCountRef.current - 1);
            if (activeSearchCountRef.current === 0) {
                setTimeout(() => { isSenderRef.current = false; }, 500);
            }
        };

        invoke("send_intent_to_mesh", { payload: submittedIntent }).catch(console.error);

        if (ceiling <= 0) {
            if (!dismissedSearchesRef.current.has(id)) {
                updateSearch(id, { status: "error", message: "Set a max AVAX (≤ field) to enable AI matching." });
            }
            finishSearch();
            return;
        }

        try {
            const match = await invoke<MatchResult | null>("match_intent_to_listings", {
                intent: submittedIntent,
                priceCeiling: ceiling,
            });

            if (dismissedSearchesRef.current.has(id)) {
                finishSearch();
                return; // user dismissed it while it was scanning — no funds were spent
            }

            if (!match) {
                updateSearch(id, { status: "no-match", message: "No matching listing within your price ceiling." });
                finishSearch();
                return;
            }

            updateSearch(id, { status: "buying", message: `⚔️ Closing the deal on voucher #${match.token_id} — locking AVAX...` });

            const result = await invoke<TxResult>("buy_listing", {
                listingId: match.listing_id,
                priceAvax: match.price_avax,
            });

            if (result.kind === "confirmed") {
                updateSearch(id, { status: "matched", message: `Deal #${result.id} created — voucher + funds locked ✓` });
                setEscrowId(result.id);
                setDealSource("arsenal");
                setDealItemLabel(`🎫 #${match.token_id} — ${match.description}`);
                setDealPriceLabel(`${(parseFloat(match.price_avax) || 0).toFixed(2)} AVAX`);
                setDealTokenId(match.token_id);
                dealTokenIdRef.current = match.token_id;
                dealSellerRef.current = match.seller;
                setDeliveredContent({});
                setView("escrow");

                // Ask whoever sold this voucher (if online) to deliver the real content.
                invoke("send_intent_to_mesh", {
                    payload: JSON.stringify({ type: "ContentRequest", token_id: match.token_id }),
                }).catch(console.error);

                setTimeout(() => {
                    setSearches(prev => prev.filter(j => j.id !== id));
                }, 1500);
            } else {
                // No network reachable — signed offline and queued for a mesh peer to relay.
                updateSearch(id, { status: "queued", message: `No network — signed offline, queued for relay` });
                setOfflineQueue((prev) => [...prev, {
                    id: result.queueId,
                    raw_tx_hex: "",
                    summary: `Buy: 🎫 #${match.token_id} — ${match.description} (${match.price_avax} AVAX)`,
                    created_at: new Date().toISOString(),
                    status: "queued",
                    tx_hash: null,
                }]);

                setTimeout(() => {
                    setSearches(prev => prev.filter(j => j.id !== id));
                }, 2500);
            }
        } catch (e) {
            console.error("AI matching / buy failed:", e);
            if (!dismissedSearchesRef.current.has(id)) {
                updateSearch(id, { status: "error", message: `Failed: ${e}` });
            }
        } finally {
            finishSearch();
        }
    };

    const handleIntentSubmit = () => {
        if (!intent.trim()) return;
        if (showHint) dismissHint();
        const submittedIntent = intent.trim();
        const ceiling = parseFloat(priceCeiling) || 0;
        setIntent(""); // clear immediately so another intent can be typed right away

        const id = ++nextSearchIdRef.current;

        // Reject up front if the max price already exceeds the real wallet balance —
        // no point scanning/matching/broadcasting an intent that can never be paid for.
        if (myBalanceAvax != null && ceiling > myBalanceAvax) {
            setSearches(prev => [...prev, {
                id,
                intent: submittedIntent,
                status: "error",
                message: `Max price ${ceiling} AVAX exceeds your wallet balance (${myBalanceAvax} AVAX).`,
            }]);
            return;
        }

        setSearches(prev => [...prev, { id, intent: submittedIntent, status: "scanning", message: "Broadcasting to mesh + scanning Arsenal listings..." }]);
        runSearch(id, submittedIntent, ceiling);
    };

    const handleReleaseFunds = () => {
        console.log("💰 Releasing funds and broadcasting settlement...");

        // The voucher NFT only transfers to the buyer at release time (see
        // Marketplace.sol's releaseDeal) — bump the shared refresh key so
        // Inventory (and Trading Post) pick up the new ownership state.
        setListingsRefreshKey((k) => k + 1);

        // Broadcast settlement confirmation back to the sender
        invoke("send_intent_to_mesh", {
            payload: JSON.stringify({
                type: "SettlementComplete",
                deal: "Fox NFT #04",
                amount: "13.5 AVAX"
            })
        }).then(() => {
            console.log("✅ Settlement confirmation broadcasted!");
        }).catch(console.error);
    };

    // Reopens Smart Escrow for a deal that's still active — needed for deals that
    // didn't route there live (e.g. one confirmed later by the offline-relay retry).
    const handleOpenDeal = (deal: DealView) => {
        setEscrowId(deal.deal_id);
        setDealSource("arsenal");
        setDealItemLabel(`🎫 #${deal.token_id}`);
        setDealPriceLabel(`${(parseFloat(deal.amount_avax) || 0).toFixed(2)} AVAX`);
        dealSellerRef.current = deal.seller;
        setView("escrow");
    };

    const handleCreateService = () => setView("service-creator");
    const handleDeployService = (listingId: number) => {
        console.log("✅ On-chain listing created:", listingId);
        setListingsRefreshKey((k) => k + 1);
        setView("nexus");
    };

    return (
        <ErrorBoundary>
            <Nexus isOnline={isOnline} peerCount={peers.length} aiReady={aiReady}>
                {/* Background mesh scene is always visible in Nexus mode */}
                <AmbientParticles />
                <MeshCharacterField peers={peers} myAddress={myAddress} />

                {/* Main Menu Bar */}
                {view === "nexus" && (
                    <div className="absolute top-4 left-4 right-4 z-50 flex flex-wrap gap-2 items-start">
                        <div className="h-8 flex items-center gap-1.5 bg-nobody-charcoal pixel-corners-sm px-3 border border-nobody-primary/20 shadow-card">
                            <img src={skullIcon} alt="" draggable={false} style={{ width: 16, height: 14, imageRendering: "pixelated" }} />
                            <span className="text-[10px] font-pixel tracking-wide text-slate-500">CABALMESH</span>
                        </div>
                        <MenuButton label="🎒" onClick={() => setView("redeem")} active={false} color="nobody-gold" />
                        <div className="w-px self-stretch bg-slate-300/60 mx-1 hidden sm:block" />
                        <RelayerStatusCompact isRelaying={isRelaying} onToggle={setIsRelaying} peerCount={peers.length} />
                        <WalletBalanceCompact onOpenWallet={() => setView("wallet-cabinet")} />
                    </div>
                )}

                {/* Always-visible listings/deals — no need to open Arsenal Mode just to look */}
                {view === "nexus" && (
                    <TradingPostPanel refreshKey={listingsRefreshKey} onCreateListing={handleCreateService} />
                )}
                {view === "nexus" && (
                    <QuestLogPanel refreshKey={listingsRefreshKey} onOpenDeal={handleOpenDeal} />
                )}

                {/* Sender-side queue of this node's own in-flight/finished intent searches */}
                {view === "nexus" && (
                    <SearchQueue jobs={searches} onDismiss={handleDismissSearch} />
                )}

                {/* Transactions signed offline, queued for a mesh peer with real connectivity to relay */}
                {view === "nexus" && (
                    <OfflineQueue items={offlineQueue} onDismiss={handleDismissOfflineItem} />
                )}

                {/* One-time onboarding hint for first-time users */}
                {view === "nexus" && (
                    <OnboardingHint visible={showHint} onDismiss={dismissHint} />
                )}

                {/* Main Command Input */}
                {view === "nexus" && (
                    <IntentComposer
                        intent={intent}
                        setIntent={setIntent}
                        priceCeiling={priceCeiling}
                        setPriceCeiling={setPriceCeiling}
                        onSubmit={handleIntentSubmit}
                        isProcessing={searches.some((j) => j.status === "scanning" || j.status === "buying")}
                    />
                )}

                {/* Overlays */}
                <NotificationToast
                    message={notification}
                    onClose={() => setNotification(null)}
                />

                {/* Wallet Integration */}
                <WalletCabinet
                    visible={view === "wallet-cabinet"}
                    onClose={() => setView("nexus")}
                    onDelegate={() => setView("delegation")}
                    peerCount={peers.length}
                />

                {view === "service-creator" && (
                    <ServiceCreator
                        onClose={() => setView("nexus")}
                        onDeploy={handleDeployService}
                    />
                )}

                <SmartEscrow // Added SmartEscrow component
                    visible={view === "escrow"}
                    escrowId={escrowId}
                    dealSource={dealSource}
                    onClose={() => setView("nexus")}
                    onRelease={handleReleaseFunds}
                    itemLabel={dealItemLabel}
                    priceLabel={dealPriceLabel}
                    deliveredContent={dealTokenId != null ? deliveredContent[dealTokenId] : undefined}
                    buyerAddress={myAddress ?? undefined}
                    sellerAddress={dealSellerRef.current ?? undefined}
                />

                <DelegationCenter
                    visible={view === "delegation"}
                    onComplete={() => setView("wallet-cabinet")}
                    onCancel={() => setView("wallet-cabinet")}
                />

                <RedeemVoucher
                    visible={view === "redeem"}
                    refreshKey={listingsRefreshKey}
                    onClose={() => setView("nexus")}
                    onListed={() => setListingsRefreshKey((k) => k + 1)}
                />
            </Nexus>
        </ErrorBoundary>
    );
}

// Helper Component for Menu Header
const MenuButton = ({ label, onClick, color }: { label: string, onClick: () => void, active: boolean, color: string }) => {
    // Determine color classes
    const colorClass = color === "nobody-primary" ? "text-nobody-primary border-nobody-primary/30 hover:bg-nobody-primary-soft" :
        color === "nobody-gold" ? "text-nobody-gold border-nobody-gold/30 hover:bg-nobody-gold-soft" :
            color === "yellow-500" ? "text-amber-600 border-amber-200 hover:bg-amber-50" :
                color === "purple-500" ? "text-nobody-gold border-nobody-gold/30 hover:bg-nobody-gold-soft" :
                    color === "green-500" ? "text-nobody-primary border-nobody-primary/30 hover:bg-nobody-primary-soft" :
                        "text-slate-500 hover:text-slate-900 border-slate-200 hover:border-slate-300";

    return (
        <button
            onClick={onClick}
            className={`text-[10px] font-pixel px-3 py-2 border transition-all tracking-wide bg-nobody-charcoal shadow-card pixel-corners-sm ${colorClass}`}
        >
            [{label}]
        </button>
    );
};

export default App;
