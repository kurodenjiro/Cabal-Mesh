/**
 * `MARKET`: browse and buy modules other nodes are selling, manage deals this
 * identity is party to, and list an owned module for sale.
 *
 * Reached from `VAULT → MODULES`'s `[ MARKET ]` button — see
 * `docs/intent-chat-and-modules-design.md`'s Marketplace mock-up. Escrow is
 * real: `buy` atomically locks AVAX and pulls the module into the Marketplace
 * contract, and only `releaseDeal`/`refundDeal` — both buyer-only, enforced
 * on-chain — ever move it again. Nothing here trusts a listing that has gone
 * stale; the buy command itself surfaces that as a clear error rather than a
 * raw revert.
 */
import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Input, Panel, Select } from "../ds";
import { errorCopy } from "../state/errorCopy";
import type { AssetListingView, DealView, ModuleCardView } from "../types/bindings";

const label: CSSProperties = {
  fontFamily: "var(--type-label-family)",
  fontSize: "var(--text-2xs)",
  letterSpacing: "var(--tracking-widest)",
  color: "var(--text-muted)",
};

export function Market() {
  const [listings, setListings] = useState<AssetListingView[] | null>(null);
  const [deals, setDeals] = useState<DealView[] | null>(null);
  const [owned, setOwned] = useState<ModuleCardView[] | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    invoke<AssetListingView[]>("market_listings").then(setListings).catch(() => setListings([]));
    invoke<DealView[]>("market_my_deals").then(setDeals).catch(() => setDeals([]));
    invoke<ModuleCardView[]>("vault_modules").then(setOwned).catch(() => setOwned([]));
  };
  useEffect(refresh, []);

  async function buy(listing: AssetListingView) {
    setBusy(listing.id);
    setError(null);
    try {
      await invoke("market_buy", { listingId: listing.id, priceWei: listing.priceWei });
      refresh();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(null);
    }
  }

  async function release(dealId: number) {
    setBusy(dealId);
    setError(null);
    try {
      await invoke("market_release_deal", { dealId });
      refresh();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(null);
    }
  }

  async function refund(dealId: number) {
    setBusy(dealId);
    setError(null);
    try {
      await invoke("market_refund_deal", { dealId });
      refresh();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(null);
    }
  }

  const activeDeals = (deals ?? []).filter((deal) => deal.status === "active");
  const unlisted = (owned ?? []).filter((module) => module.moduleType.length > 0);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)" }}>
      <Panel label={`LISTINGS · ${listings?.length ?? 0}`}>
        {listings === null ? null : listings.length === 0 ? (
          <div style={{ padding: "var(--space-9) var(--space-6)", textAlign: "center" }}>
            <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>
              Nothing listed for sale right now.
            </span>
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column" }}>
            {listings.map((listing) => (
              <div
                key={listing.id}
                className="cm-row"
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: "var(--space-5)",
                  padding: "var(--space-5) var(--space-6)",
                  borderTop: "var(--border-hairline-style)",
                }}
              >
                <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)", minWidth: 0 }}>
                  <span style={{ fontFamily: "var(--type-heading-family)", color: "var(--text-primary)" }}>
                    {listing.description}
                  </span>
                  <span style={label}>
                    SELLER {listing.seller.slice(0, 6)}…{listing.seller.slice(-4)} · #{listing.tokenId}
                  </span>
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: "var(--space-4)", flexShrink: 0 }}>
                  <span style={{ fontFamily: "var(--type-data-family)", color: "var(--text-primary)" }}>
                    {listing.priceAvax} AVAX
                  </span>
                  <Button
                    tone="secondary"
                    size="sm"
                    className="cm-touch"
                    disabled={busy !== null}
                    onClick={() => void buy(listing)}
                  >
                    {busy === listing.id ? "…" : "BUY"}
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </Panel>

      <Panel label={`MY DEALS · ${activeDeals.length} ACTIVE`}>
        {deals === null ? null : deals.length === 0 ? (
          <div style={{ padding: "var(--space-9) var(--space-6)", textAlign: "center" }}>
            <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>
              No deals yet — buy or sell to start one.
            </span>
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column" }}>
            {deals.map((deal) => (
              <div
                key={deal.dealId}
                className="cm-row"
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: "var(--space-5)",
                  padding: "var(--space-5) var(--space-6)",
                  borderTop: "var(--border-hairline-style)",
                }}
              >
                <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
                  <span style={{ fontFamily: "var(--type-data-family)", color: "var(--text-primary)" }}>
                    #{deal.tokenId} · {deal.amountAvax} AVAX
                  </span>
                  <span style={label}>
                    {deal.role.toUpperCase()} · {deal.status.toUpperCase()}
                  </span>
                </div>
                {deal.status === "active" && deal.role === "buyer" && (
                  <div style={{ display: "flex", gap: "var(--space-3)", flexShrink: 0 }}>
                    <Button
                      tone="ghost"
                      size="sm"
                      className="cm-touch"
                      disabled={busy !== null}
                      onClick={() => void refund(deal.dealId)}
                    >
                      {busy === deal.dealId ? "…" : "REFUND"}
                    </Button>
                    <Button
                      tone="secondary"
                      size="sm"
                      className="cm-touch"
                      disabled={busy !== null}
                      onClick={() => void release(deal.dealId)}
                    >
                      {busy === deal.dealId ? "…" : "RELEASE"}
                    </Button>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </Panel>

      <SellPanel modules={unlisted} onListed={refresh} />

      {error && (
        <div style={{ padding: "0 var(--space-6)" }}>
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-wide)",
              color: "var(--accent-blood-red)",
              textTransform: "uppercase",
            }}
          >
            {error}
          </span>
        </div>
      )}
    </div>
  );
}

/**
 * `LIST ON MARKET`: two on-chain steps (approve, then list) behind one
 * button — see `BlockchainBridge::create_asset_listing`'s doc comment.
 */
function SellPanel({ modules, onListed }: { modules: ModuleCardView[]; onListed: () => void }) {
  const [tokenId, setTokenId] = useState("");
  const [price, setPrice] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function list() {
    const selected = modules.find((m) => String(m.tokenId) === tokenId);
    if (!selected || !price) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("market_list_module", {
        tokenId: selected.tokenId,
        description: selected.moduleType,
        priceAvax: price,
      });
      setTokenId("");
      setPrice("");
      onListed();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel label="SELL A MODULE">
      <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
        {modules.length === 0 ? (
          <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>
            Nothing owned that isn't already listed.
          </span>
        ) : (
          <>
            <Select
              value={tokenId}
              onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setTokenId(e.target.value)}
              placeholder="Choose a module"
              options={modules.map((m) => ({ value: String(m.tokenId), label: `${m.moduleType} · #${m.tokenId}` }))}
            />
            <Input
              value={price}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => setPrice(e.target.value)}
              inputMode="decimal"
              pattern="[0-9]*[.,]?[0-9]*"
              autoComplete="off"
              placeholder="Price in AVAX"
            />
            <Button
              tone="secondary"
              size="sm"
              className="cm-touch"
              disabled={busy || !tokenId || !price}
              onClick={() => void list()}
            >
              {busy ? "…" : "LIST ON MARKET"}
            </Button>
          </>
        )}
        {error && (
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-wide)",
              color: "var(--accent-blood-red)",
              textTransform: "uppercase",
            }}
          >
            {error}
          </span>
        )}
      </div>
    </Panel>
  );
}
