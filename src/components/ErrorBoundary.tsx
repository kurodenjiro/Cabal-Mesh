import { Component, ErrorInfo, ReactNode } from "react";

interface Props {
    children?: ReactNode;
}

interface State {
    hasError: boolean;
    error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
    public state: State = {
        hasError: false,
        error: null,
    };

    public static getDerivedStateFromError(error: Error): State {
        return { hasError: true, error };
    }

    public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
        console.error("Uncaught error:", error, errorInfo);
    }

    public render() {
        if (this.state.hasError) {
            return (
                <div className="p-8 bg-red-950 text-slate-900 font-sans h-screen flex flex-col items-center justify-center">
                    <h1 className="text-2xl font-bold mb-4 text-red-400">⚠️ System Crash Detected</h1>
                    <div className="bg-nobody-charcoal p-4 rounded-2xl border border-red-800/50 shadow-card max-w-2xl overflow-auto">
                        <p className="font-mono text-red-400 text-sm">{this.state.error?.toString()}</p>
                    </div>
                    <button
                        className="mt-8 px-6 py-2 bg-red-600 text-white font-semibold rounded-xl hover:bg-red-700 transition-colors"
                        onClick={() => window.location.reload()}
                    >
                        System Reboot
                    </button>
                </div>
            );
        }

        return this.props.children;
    }
}
