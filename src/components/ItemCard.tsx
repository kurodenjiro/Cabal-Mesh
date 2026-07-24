import React from "react";
import { priceRarity } from "../lib/rpgFlavor";

interface ItemCardProps {
    icon?: React.ReactNode;
    title: string;
    subtitle?: string;
    priceAvax: number;
    priceLabel?: string; // defaults to `${priceAvax} AVAX`
    onClick?: () => void;
    selected?: boolean;
    /** Set false when there's no real price backing this item (e.g. an
     * already-owned voucher) — hides the rarity tier instead of showing a
     * misleading tier derived from a placeholder price. */
    showRarity?: boolean;
}

/** A reusable RPG-style item card — icon, name, rarity-colored border/ribbon
 * derived from the real price, and the price itself. Used everywhere a
 * listing or voucher is shown (Arsenal, Redeem, Smart Escrow). */
export const ItemCard: React.FC<ItemCardProps> = ({ icon = "🎫", title, subtitle, priceAvax, priceLabel, onClick, selected, showRarity = true }) => {
    const rarity = priceRarity(priceAvax);
    const Wrapper = onClick ? "button" : "div";
    const borderClass = showRarity ? rarity.borderClass : "border-slate-200";
    const accentClass = showRarity ? rarity.textClass : "text-nobody-gold";

    return (
        <Wrapper
            onClick={onClick}
            className={`w-full text-left bg-slate-50 pixel-corners-sm border-2 p-3 flex items-center gap-3 transition-colors ${borderClass} ${selected ? "bg-nobody-primary-soft/30" : onClick ? "hover:bg-slate-100" : ""}`}
        >
            <span className="text-xl shrink-0">{icon}</span>
            <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                    <span className="text-slate-900 font-semibold text-sm truncate">{title}</span>
                    {showRarity && <span className={`text-[9px] font-pixel tracking-wide uppercase shrink-0 ${rarity.textClass}`}>{rarity.tier}</span>}
                </div>
                {subtitle && <div className="text-slate-400 text-[11px] truncate">{subtitle}</div>}
            </div>
            <span className={`text-xs font-semibold shrink-0 ${accentClass}`}>{priceLabel ?? `${priceAvax.toFixed(5)} AVAX`}</span>
        </Wrapper>
    );
};

export default ItemCard;
