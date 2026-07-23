import React from "react";
import scene from "../assets/background/scene.png";
import circleIcon from "../assets/icons/icon_circle.png";
import peerIcon from "../assets/icons/icon_peer.png";
import zzzIcon from "../assets/icons/icon_zzz.png";

interface StatusIndicatorProps {
    isOnline: boolean;
    peerCount: number;
    aiReady: boolean;
}

export const Nexus: React.FC<React.PropsWithChildren<StatusIndicatorProps>> = ({ children, isOnline, peerCount, aiReady }) => {
    return (
        <div className="h-screen w-screen bg-nobody-dark overflow-hidden flex flex-col relative font-sans text-slate-900 selection:bg-nobody-primary-soft selection:text-nobody-ink">
            {/* Pixel-art RPG night-ruins backdrop, purely decorative. */}
            <div
                className="absolute inset-0 pointer-events-none"
                style={{
                    backgroundImage: `url(${scene})`,
                    backgroundSize: "cover",
                    backgroundPosition: "center",
                    imageRendering: "pixelated",
                }}
            />

            <div className="absolute top-4 right-4 flex gap-2 z-30">
                <StatusItem
                    icon={
                        <img
                            src={circleIcon}
                            alt={isOnline ? "Online" : "Offline"}
                            draggable={false}
                            style={{
                                width: 12,
                                height: 12,
                                imageRendering: "pixelated",
                                filter: isOnline ? "brightness(1.15)" : "grayscale(1) brightness(0.55)",
                            }}
                        />
                    }
                    title={isOnline ? "Online" : "Offline"}
                />

                <StatusItem
                    icon={<img src={peerIcon} alt="Peers" draggable={false} style={{ width: 14, height: 12, imageRendering: "pixelated" }} />}
                    label={`${peerCount}`}
                    title={`${peerCount} peer${peerCount === 1 ? "" : "s"} connected`}
                />

                <StatusItem
                    icon={
                        <img
                            src={zzzIcon}
                            alt={aiReady ? "AI ready" : "AI offline"}
                            draggable={false}
                            style={{
                                width: 14,
                                height: 12,
                                imageRendering: "pixelated",
                                filter: aiReady ? "brightness(1.15)" : "grayscale(1) brightness(0.55)",
                            }}
                        />
                    }
                    title={aiReady ? "AI ready" : "AI offline"}
                />
            </div>

            {/* Main Content Area */}
            {children}
        </div>
    );
};

const StatusItem = ({ icon, label, title }: { icon: React.ReactNode, label?: string, title?: string }) => (
    <div title={title} className="h-8 flex items-center gap-1 bg-nobody-charcoal pixel-corners-sm px-3 border border-nobody-primary/20 justify-center">
        <span className="text-xs flex items-center">{icon}</span>
        {label && <span className="text-[10px] font-pixel tracking-wide text-slate-500">{label}</span>}
    </div>
);
