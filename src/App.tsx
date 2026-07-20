import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

// Components
import { Nexus } from "./components/Nexus";
import { MeshRadar } from "./components/MeshRadar";
import { IntentComposer } from "./components/IntentComposer";
import { AgentLog } from "./components/AgentLog";
import { SmartMeshChat } from "./components/SmartMeshChat";
import { IntegrityShield } from "./components/IntegrityShield";
import { DailyReport } from "./components/DailyReport";
import { NodeConfig } from "./components/NodeConfig";
import { NotificationToast } from "./components/NotificationToast";
import { ProviderDashboard } from "./components/ProviderDashboard";
import { ServiceCreator } from "./components/ServiceCreator";
import { ShieldedWallet } from "./components/ShieldedWallet";
import { IdentityInitialization } from "./components/IdentityInitialization";
import { WalletCabinet } from "./components/WalletCabinet";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Archives } from "./components/Archives";
import { SmartEscrow } from "./components/SmartEscrow";
import { ConfidentialCompute } from "./components/ConfidentialCompute";
import { DealNotification } from "./components/DealNotification";
import { DelegationCenter } from "./components/DelegationCenter";

// Types
import { Peer, MeshEvent, ViewState } from "./types";

function App() {
    const [intent, setIntent] = useState("");
    const [peers, setPeers] = useState<Peer[]>([]);
    const [isOnline, setIsOnline] = useState(navigator.onLine);
    const [isProcessing, setIsProcessing] = useState(false);

    // UI State
    const [view, setView] = useState<ViewState>("nexus");
    const [showConfig, setShowConfig] = useState(false);
    const [logs, setLogs] = useState<string[]>([]);
    const [notification, setNotification] = useState<string | null>(null);
    const [chatContext] = useState<any>(null);
    const [isSender, setIsSender] = useState(false); // Track if this node sent the intent
    const [escrowId, setEscrowId] = useState<number | null>(null);

    useEffect(() => {
        const unlisten = listen<MeshEvent>("mesh-event", (event) => {
            console.log("🔔 Mesh event received:", event.payload);
            const meshEvent = event.payload;

            if (meshEvent.type === "PeerDiscovered") {
                console.log("✅ PeerDiscovered event");
                setPeers((prev) => [...prev, {
                    id: meshEvent.peer_id || "",
                    address: meshEvent.address || "",
                    timestamp: Date.now(),
                }]);
            } else if (meshEvent.type === "IntentReceived") {
                console.log("✅ IntentReceived event detected!", meshEvent);
                console.log("🔍 isSender:", isSender);

                // Only show deal notification if this node is NOT the sender
                if (!isSender) {
                    console.log("📥 This node is the RECEIVER - showing deal notification");
                    // Show agent processing on receiving node
                    setIsProcessing(true);
                    setLogs([]);
                    addLog(`→ Encrypted intent received from peer`);

                    setTimeout(() => {
                        addLog(`→ Decrypting ZK-Proof package...`);
                    }, 500);

                    setTimeout(() => {
                        addLog(`→ Verifying proof integrity...`);
                    }, 1200);

                    setTimeout(() => {
                        addLog(`→ Match found! Preparing deal...`);
                    }, 2000);

                    // Show deal notification after processing
                    setTimeout(() => {
                        setIsProcessing(false);
                        setLogs([]);
                        setNotification("📥 New deal opportunity!");
                        setView("notification");
                    }, 3000);
                } else {
                    console.log("📤 This node is the SENDER - ignoring own broadcast");
                }

            } else if (meshEvent.type === "DealAccepted") {
                console.log("✅ Deal accepted by peer!", meshEvent);
                setNotification("🤝 Deal accepted! Waiting for settlement...");

            } else if (meshEvent.type === "SettlementComplete") {
                console.log("✅ Settlement confirmation received!", meshEvent);
                setNotification("✅ Funds released! Transaction complete.");
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

    const addLog = (msg: string) => {
        setLogs(prev => [...prev, msg]);
    };

    const handleIntentSubmit = async () => {
        if (!intent.trim()) return;

        setIsProcessing(true);
        setLogs([]); // Clear previous logs
        setIsSender(true); // Mark this node as the sender

        addLog(`→ Analyzing prompt: "${intent}"`);

        setTimeout(() => {
            addLog(`→ Generating Noir ZK-Proof...`);
        }, 800);

        setTimeout(() => {
            addLog(`→ ZK-Proof Complete ✓`);
        }, 2000);

        setTimeout(() => {
            addLog(`→ CONFIDENTIAL COMPUTE: Broadcasting encrypted intent to mesh...`);
            // Broadcast to mesh network
            invoke("send_intent_to_mesh", { payload: intent }).catch(console.error);
        }, 2500);

        setTimeout(() => {
            addLog(`→ Waiting for peer responses...`);
        }, 3200);

        // Sender waits for responses, doesn't auto-show notification
        setTimeout(() => {
            setIsProcessing(false);
            setLogs([]);
            addLog(`→ Broadcast complete. Waiting for matches...`);
            // Reset sender flag after broadcast completes
            setTimeout(() => setIsSender(false), 1000);
        }, 5000);
    };

    const handleAcceptDeal = async () => {
        console.log("🤝 Accepting deal and broadcasting to sender...");
        // Broadcast deal acceptance to the sender
        invoke("send_intent_to_mesh", {
            payload: JSON.stringify({
                type: "DealAccepted",
                deal: "Fox NFT #04",
                price: "13.5 AVAX"
            })
        }).then(() => {
            console.log("✅ Deal acceptance broadcasted!");
        }).catch(console.error);

        // Lock a small real Fuji-testnet amount in the on-chain Escrow contract.
        // Note: the mesh protocol doesn't yet exchange peers' on-chain addresses,
        // so this demo escrows to the buyer's own primary identity as payee.
        try {
            const identities = await invoke<{ address: string }[]>("get_identity");
            const payee = identities[0]?.address;
            if (payee) {
                const id = await invoke<number>("create_escrow", { payee, amountAvax: "0.01" });
                console.log("✅ On-chain escrow created:", id);
                setEscrowId(id);
            }
        } catch (e) {
            console.error("Failed to create on-chain escrow:", e);
            setEscrowId(null);
        }

        setView("escrow");
    };

    const handleReleaseFunds = () => {
        console.log("💰 Releasing funds and broadcasting settlement...");
        // Hide logs/processing UI
        setIsProcessing(false);
        setLogs([]);

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

    const handleSwitchToProvider = () => setView("provider");
    const handleCreateService = () => setView("service-creator");
    const handleDeployService = (config: any) => {
        console.log("Deploying service:", config);
        setTimeout(() => setView("provider"), 1000);
    };

    return (
        <ErrorBoundary>
            <Nexus isOnline={isOnline} peerCount={peers.length} showConfig={() => setShowConfig(true)}>
                {/* Background Radar is always visible in Nexus mode */}
                <MeshRadar peers={peers} />

                {/* Main Menu Bar */}
                {view === "nexus" && (
                    <div className="absolute top-4 left-4 z-50 flex gap-2">
                        <MenuButton label="MERCHANT" onClick={handleSwitchToProvider} active={false} color="nobody-mint" />
                        <MenuButton label="WALLET" onClick={() => setView("wallet-cabinet")} active={false} color="nobody-mint" />
                    </div>
                )}

                {/* Live System Logs */}
                <AgentLog visible={(logs.length > 0 || isProcessing) && view === "nexus"} logs={logs} />

                {/* Main Command Input */}
                {view === "nexus" && (
                    <IntentComposer
                        intent={intent}
                        setIntent={setIntent}
                        onSubmit={handleIntentSubmit}
                        isProcessing={isProcessing}
                    />
                )}

                {/* Overlays */}
                <NotificationToast
                    message={notification}
                    onClose={() => setNotification(null)}
                />

                <IntegrityShield
                    visible={view === "integrity"}
                    onComplete={() => { }}
                />

                <SmartMeshChat
                    visible={view === "negotiation"}
                    onComplete={() => setView("report")}
                    context={chatContext}
                />

                <DailyReport
                    visible={view === "report"}
                    onClose={() => setView("nexus")}
                />

                <NodeConfig
                    visible={showConfig}
                    onClose={() => setShowConfig(false)}
                />

                {/* Provider Components */}
                {/* Provider Components */}
                <ProviderDashboard
                    visible={view === "provider"}
                    onClose={() => setView("nexus")}
                    onCreateService={handleCreateService}
                />

                {/* Wallet Integration */}
                <WalletCabinet
                    visible={view === "wallet-cabinet"}
                    onClose={() => setView("nexus")}
                    onOpenConfig={() => setShowConfig(true)}
                    onAddNew={() => setView("identity-init")}
                    onDelegate={() => setView("delegation")}
                />

                {view === "identity-init" && (
                    <IdentityInitialization
                        onComplete={() => setView("wallet-cabinet")}
                        onBack={() => setView("nexus")}
                    />
                )}

                {view === "service-creator" && (
                    <ServiceCreator
                        onClose={() => setView("provider")}
                        onDeploy={handleDeployService}
                    />
                )}

                <ShieldedWallet
                    visible={view === "wallet"}
                    onClose={() => setView("nexus")}
                />

                <Archives // Rendered Archives component
                    visible={view === "archives"}
                    onClose={() => setView("nexus")}
                />

                <SmartEscrow // Added SmartEscrow component
                    visible={view === "escrow"}
                    escrowId={escrowId}
                    onClose={() => setView("nexus")}
                    onRelease={handleReleaseFunds}
                />

                <ConfidentialCompute
                    visible={view === "compute"}
                    onClose={() => setView("nexus")}
                />

                <DealNotification
                    visible={view === "notification"}
                    onClose={() => setView("nexus")}
                    onAccept={handleAcceptDeal}
                />

                <DelegationCenter
                    visible={view === "delegation"}
                    onComplete={() => setView("wallet-cabinet")}
                    onCancel={() => setView("wallet-cabinet")}
                />
            </Nexus>
        </ErrorBoundary>
    );
}

// Helper Component for Menu Header
const MenuButton = ({ label, onClick, color }: { label: string, onClick: () => void, active: boolean, color: string }) => {
    // Determine color classes
    const colorClass = color === "nobody-mint" ? "text-nobody-mint border-nobody-mint/30 hover:bg-nobody-mint-soft" :
        color === "nobody-violet" ? "text-nobody-violet border-nobody-violet/30 hover:bg-nobody-violet-soft" :
            color === "yellow-500" ? "text-amber-400 border-amber-800/40 hover:bg-amber-950/40" :
                color === "purple-500" ? "text-nobody-violet border-nobody-violet/30 hover:bg-nobody-violet-soft" :
                    color === "green-500" ? "text-nobody-mint border-nobody-mint/30 hover:bg-nobody-mint-soft" :
                        "text-slate-500 hover:text-slate-900 border-slate-200 hover:border-slate-300";

    return (
        <button
            onClick={onClick}
            className={`text-xs font-semibold px-3 py-1.5 border rounded-full transition-all tracking-wide bg-nobody-charcoal shadow-card ${colorClass}`}
        >
            {label}
        </button>
    );
};

export default App;
