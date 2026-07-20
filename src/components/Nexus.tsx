import React from "react";

interface StatusIndicatorProps {
    isOnline: boolean;
    peerCount: number;
    showConfig: () => void;
}

export const Nexus: React.FC<React.PropsWithChildren<StatusIndicatorProps>> = ({ children, showConfig }) => {
    return (
        <div className="h-screen w-screen bg-nobody-dark overflow-hidden flex flex-col relative font-sans text-slate-900 selection:bg-nobody-mint-soft selection:text-nobody-ink">
            <div className="absolute top-4 right-4 flex gap-2 z-30">
                <StatusItem
                    icon="⚡"
                    label="4ms"
                />

                <StatusItem
                    icon="🌐"
                    label="Node: Alpha-7"
                />

                <StatusItem
                    icon="👤"
                    label="98% Rep"
                    textColor="text-nobody-mint"
                />

                <button
                    onClick={showConfig}
                    className="h-8 bg-white px-4 rounded-full border border-slate-200 shadow-card text-slate-500 hover:text-slate-900 hover:border-slate-300 transition-all text-xs font-medium tracking-wide"
                >
                    Config
                </button>
            </div>

            {/* Main Content Area */}
            {children}
        </div>
    );
};

const StatusItem = ({ icon, label, textColor = "text-slate-600" }: { icon: string, label: string, textColor?: string }) => (
    <div className="h-8 flex items-center gap-2 bg-white px-3 rounded-full border border-slate-200 shadow-card min-w-[100px] justify-center">
        <span className="text-xs">{icon}</span>
        <span className={`text-xs font-medium tracking-wide ${textColor}`}>
            {label}
        </span>
    </div>
);
