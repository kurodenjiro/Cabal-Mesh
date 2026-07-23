import React, { useState } from "react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { ContentRecord } from "../types";
import aiComputeIcon from "../assets/items/ai_compute_credit.png";
import relayBandwidthIcon from "../assets/items/relay_bandwidth_credit.png";
import bookPageIcon from "../assets/items/book_page.png";

interface ServiceCreatorProps {
    onClose: () => void;
    onDeploy: (listingId: number) => void;
}

const VOUCHER_TYPES = ["AI Compute Credit", "Relay Bandwidth Credit", "Book Page (PDF)", "Custom"];
const PDF_TYPE = "Book Page (PDF)";
const VOUCHER_TYPE_ICON: Partial<Record<string, string>> = {
    "AI Compute Credit": aiComputeIcon,
    "Relay Bandwidth Credit": relayBandwidthIcon,
    "Book Page (PDF)": bookPageIcon,
};

type Step = "idle" | "extracting" | "minting" | "approving" | "listing" | "signing" | "done";

const STEP_LABEL: Record<Step, string> = {
    idle: "",
    extracting: "Extracting page 1 text from the PDF...",
    minting: "Minting item NFT (proof of possession)...",
    approving: "Approving Marketplace to hold the item...",
    listing: "Creating on-chain listing...",
    signing: "Signing the page content (real signature, not ZK)...",
    done: "Listed ✓",
};

export const ServiceCreator: React.FC<ServiceCreatorProps> = ({ onClose, onDeploy }) => {
    const [voucherType, setVoucherType] = useState(VOUCHER_TYPES[0]);
    const [customType, setCustomType] = useState("");
    const [description, setDescription] = useState("");
    const [priceAvax, setPriceAvax] = useState("");
    const [step, setStep] = useState<Step>("idle");
    const [error, setError] = useState<string | null>(null);

    // PDF-specific state
    const [pdfFileName, setPdfFileName] = useState<string | null>(null);
    const [extractedText, setExtractedText] = useState("");
    const [extracting, setExtracting] = useState(false);

    const isPdfListing = voucherType === PDF_TYPE;
    const effectiveType = voucherType === "Custom" ? customType.trim() : voucherType;
    const isDeploying = step !== "idle" && step !== "done";
    const hasContent = isPdfListing ? extractedText.trim().length > 0 : description.trim().length > 0;
    const canDeploy = effectiveType.length > 0 && hasContent && parseFloat(priceAvax) > 0 && !isDeploying && !extracting;

    const handlePdfSelect = async (file: File) => {
        setPdfFileName(file.name);
        setExtractedText("");
        setError(null);
        setExtracting(true);
        try {
            const buffer = await file.arrayBuffer();
            const bytes = Array.from(new Uint8Array(buffer));
            const text = await invoke<string>("extract_pdf_text", { pdfBytes: bytes });
            setExtractedText(text);
        } catch (e) {
            console.error("Failed to extract PDF text:", e);
            setError("Failed to read PDF: " + e);
        } finally {
            setExtracting(false);
        }
    };

    const handleDeploy = async () => {
        if (!canDeploy) return;
        setError(null);
        try {
            const content = isPdfListing ? extractedText.trim() : description.trim();
            const listingDescription = isPdfListing
                ? (content.length > 100 ? content.slice(0, 100) + "..." : content)
                : content;

            setStep("minting");
            const tokenId = await invoke<number>("mint_voucher", {
                voucherType: effectiveType,
                description: listingDescription,
            });

            setStep("approving");
            await invoke("approve_voucher", { tokenId });

            setStep("listing");
            const listingId = await invoke<number>("create_asset_listing", {
                description: listingDescription,
                priceAvax,
                tokenId,
            });

            if (isPdfListing) {
                setStep("signing");
                const record = await invoke<ContentRecord>("sign_content", { text: content });
                await invoke("store_content", { tokenId, record });
            }

            setStep("done");
            onDeploy(listingId);
        } catch (e) {
            console.error("Failed to create asset-backed listing:", e);
            setError("On-chain listing failed: " + e);
            setStep("idle");
        }
    };

    return (
        <motion.div
            className="absolute inset-0 bg-black/70 backdrop-blur-sm z-50 flex items-center justify-center p-8"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
        >
            <div className="w-full max-w-2xl pixel-corners border border-slate-200 bg-nobody-charcoal shadow-card-lg overflow-hidden flex flex-col">

                {/* Header */}
                <div className="bg-slate-50 p-3 border-b border-slate-200 flex justify-between items-center">
                    <span className="text-nobody-gold font-pixel tracking-wide text-[10px]">[ ✨ CREATE NEW LISTING ]</span>
                    <div className="flex gap-4 text-xs">
                        <span className="text-slate-500">Mode: Trading Post</span>
                        <span className="text-nobody-primary font-medium">🛡️ On-chain: Fuji</span>
                    </div>
                </div>

                <div className="p-6 space-y-6">

                    {/* Voucher Type */}
                    <div className="space-y-2">
                        <label className="text-slate-500 text-xs font-semibold">Item Type (mints a real NFT — proves you're the one offering it)</label>
                        <div className="flex gap-2">
                            {VOUCHER_TYPES.map((t) => (
                                <button
                                    key={t}
                                    type="button"
                                    onClick={() => setVoucherType(t)}
                                    disabled={isDeploying}
                                    className={`flex-1 flex flex-col items-center gap-1 text-[11px] font-semibold py-2 px-1 pixel-corners-sm border transition-colors ${voucherType === t ? "border-nobody-primary bg-nobody-primary-soft text-nobody-primary" : "border-slate-200 text-slate-500 hover:border-slate-300"}`}
                                >
                                    {VOUCHER_TYPE_ICON[t] && (
                                        <img src={VOUCHER_TYPE_ICON[t]} alt="" draggable={false} style={{ width: 20, height: 20, imageRendering: "pixelated" }} />
                                    )}
                                    {t}
                                </button>
                            ))}
                        </div>
                        {voucherType === "Custom" && (
                            <input
                                type="text"
                                value={customType}
                                onChange={(e) => setCustomType(e.target.value)}
                                disabled={isDeploying}
                                placeholder="Custom item type name"
                                className="w-full bg-slate-50 border border-slate-200 pixel-corners-sm p-3 text-sm text-slate-900 focus:border-nobody-primary focus:bg-nobody-charcoal outline-none transition-colors"
                            />
                        )}
                    </div>

                    {/* Content Input: PDF upload vs free-text description */}
                    {isPdfListing ? (
                        <div className="space-y-2">
                            <label className="text-slate-500 text-xs font-semibold">Upload a PDF — page 1's text is what you're selling</label>
                            <input
                                type="file"
                                accept="application/pdf"
                                disabled={isDeploying}
                                onChange={(e) => e.target.files?.[0] && handlePdfSelect(e.target.files[0])}
                                className="w-full text-xs text-slate-500 file:mr-3 file:py-2 file:px-3 file:pixel-corners-sm file:border-0 file:bg-nobody-primary file:text-nobody-ink file:text-xs file:font-semibold file:cursor-pointer"
                            />
                            {pdfFileName && (
                                <div className="text-[11px] text-slate-400">📄 {pdfFileName}</div>
                            )}
                            {extracting && (
                                <div className="text-xs text-nobody-primary animate-pulse">Reading page 1...</div>
                            )}
                            {extractedText && (
                                <div className="space-y-1">
                                    <label className="text-slate-500 text-[11px]">Extracted page 1 (this is exactly what gets signed and delivered):</label>
                                    <div className="w-full max-h-32 overflow-y-auto bg-slate-50 border border-slate-200 pixel-corners-sm p-3 text-xs text-slate-700 whitespace-pre-wrap">
                                        {extractedText}
                                    </div>
                                </div>
                            )}
                        </div>
                    ) : (
                        <div className="space-y-2">
                            <label className="text-slate-500 text-xs font-semibold">What are you offering?</label>
                            <textarea
                                value={description}
                                onChange={(e) => setDescription(e.target.value)}
                                disabled={isDeploying}
                                placeholder='Example: "1 hour of Ollama compute time over the mesh"'
                                className="w-full h-24 bg-slate-50 border border-slate-200 pixel-corners-sm p-3 text-sm text-slate-900 focus:border-nobody-primary focus:bg-nobody-charcoal outline-none resize-none transition-colors"
                            />
                        </div>
                    )}

                    {/* Price Input */}
                    <div className="space-y-2">
                        <label className="text-slate-500 text-xs font-semibold">Price (AVAX)</label>
                        <input
                            type="number"
                            min="0"
                            step="0.001"
                            value={priceAvax}
                            onChange={(e) => setPriceAvax(e.target.value)}
                            disabled={isDeploying}
                            placeholder="0.1"
                            className="w-full bg-slate-50 border border-slate-200 pixel-corners-sm p-3 text-sm text-slate-900 focus:border-nobody-primary focus:bg-nobody-charcoal outline-none transition-colors"
                        />
                    </div>

                    {hasContent && parseFloat(priceAvax) > 0 && step === "idle" && (
                        <div className="bg-nobody-gold-soft pixel-corners-sm p-3 text-center text-sm text-nobody-gold font-semibold">
                            Preview: "{effectiveType}" — {(parseFloat(priceAvax) || 0).toFixed(2)} AVAX
                        </div>
                    )}

                    {isDeploying && (
                        <div className="border-l-2 border-nobody-primary pl-4 space-y-1">
                            <div className="text-nobody-primary font-semibold text-xs animate-pulse">
                                {STEP_LABEL[step]}
                            </div>
                            <div className="text-slate-400 text-[11px]">
                                {isPdfListing ? "3 on-chain transactions + 1 content signature: mint → approve → list → sign" : "3 real on-chain transactions: mint → approve → list"}
                            </div>
                        </div>
                    )}

                    {error && (
                        <div className="text-red-600 text-xs">{error}</div>
                    )}
                </div>

                {/* Footer Actions */}
                <div className="border-t border-slate-200 p-4 flex justify-between bg-slate-50">
                    <button onClick={onClose} disabled={isDeploying} className="text-slate-400 hover:text-slate-700 transition-colors text-xs font-semibold disabled:opacity-50">🗑️ Discard</button>
                    <button
                        onClick={handleDeploy}
                        disabled={!canDeploy}
                        className={`text-white text-xs font-semibold px-4 py-2 pixel-corners-sm transition-colors ${canDeploy ? 'bg-nobody-primary hover:brightness-125' : 'bg-slate-200 text-slate-400 cursor-not-allowed'}`}
                    >
                        {isDeploying ? STEP_LABEL[step] : "🚀 Mint & List"}
                    </button>
                </div>

            </div>
        </motion.div>
    );
};
