import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Input, Panel, Select } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import type {
  ModuleDealActionView,
  ModuleDealCatalog,
  ModuleDealView,
  ModuleMarketCatalog,
  ModuleMarketListing,
  ModulePurchaseStateView,
  ModuleSlot,
  SellerStandingUnknownReason,
  SellerStandingView,
} from "../types/bindings";

type MarketFilter = "ALL" | Exclude<ModuleSlot, "NONE">;
type MarketSort = "PRICE_ASC" | "PRICE_DESC" | "RARITY_DESC" | "NAME_ASC";

const FILTERS: MarketFilter[] = ["ALL", "RADIO", "CRYPTO", "POWER"];
const SORT_OPTIONS = [
  { value: "PRICE_ASC", label: "PRICE · LOW TO HIGH" },
  { value: "PRICE_DESC", label: "PRICE · HIGH TO LOW" },
  { value: "RARITY_DESC", label: "RARITY · HIGH TO LOW" },
  { value: "NAME_ASC", label: "NAME · A TO Z" },
];

const RARITY_RANK = {
  COMMON: 0,
  RARE: 1,
  EPIC: 2,
  LEGENDARY: 3,
} as const;

/**
 * Active authentic module listings.
 *
 * The command has already pinned one accepted block, rejected stale token
 * ownership/approval, parsed canonical module fields, and independently
 * verified seller standing. This screen never reads a seller description and
 * never substitutes the device-local settlement counter for public evidence.
 */
export function Market() {
  const [catalog, setCatalog] = useState<ModuleMarketCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [request, setRequest] = useState(0);
  const [filter, setFilter] = useState<MarketFilter>("ALL");
  const [sort, setSort] = useState<MarketSort>("PRICE_ASC");
  const [search, setSearch] = useState("");
  const [browserOnline, setBrowserOnline] = useState(() => navigator.onLine);
  const [deals, setDeals] = useState<ModuleDealCatalog | null>(null);
  const [dealsLoading, setDealsLoading] = useState(true);
  const [purchaseTarget, setPurchaseTarget] = useState<ModuleMarketListing | null>(null);
  const [purchase, setPurchase] = useState<ModulePurchaseStateView | null>(null);
  const [purchaseLoading, setPurchaseLoading] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const purchaseNonce = useRef(0);

  const refresh = useCallback(() => setRequest((value) => value + 1), []);

  useEffect(() => {
    const online = () => setBrowserOnline(true);
    const offline = () => setBrowserOnline(false);
    window.addEventListener("online", online);
    window.addEventListener("offline", offline);
    return () => {
      window.removeEventListener("online", online);
      window.removeEventListener("offline", offline);
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setCatalog(null);
    invoke<ModuleMarketCatalog>("market_modules")
      .then((next) => {
        if (!cancelled) setCatalog(next);
      })
      .catch(() => {
        if (!cancelled) {
          setCatalog({
            status: "rpc_failure",
            verifiedBlock: null,
            listings: [],
            staleListings: 0,
            malformedMetadata: 0,
          });
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [request]);

  useEffect(() => {
    let cancelled = false;
    setDealsLoading(true);
    invoke<ModuleDealCatalog>("module_deals")
      .then((next) => {
        if (!cancelled) setDeals(next);
      })
      .catch(() => {
        if (!cancelled) {
          setDeals({ status: "chain_unavailable", verifiedBlock: null, observedAt: null, deals: [] });
        }
      })
      .finally(() => {
        if (!cancelled) setDealsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [request]);

  const openPurchase = useCallback((listing: ModuleMarketListing) => {
    const nonce = ++purchaseNonce.current;
    setPurchaseTarget(listing);
    setPurchase(null);
    setFeedback(null);
    setPurchaseLoading(true);
    invoke<ModulePurchaseStateView>("module_purchase_quote", { listingId: listing.listingId })
      .then((next) => {
        if (purchaseNonce.current === nonce) setPurchase(next);
      })
      .catch(() => {
        if (purchaseNonce.current === nonce) setPurchase({ status: "chain_unavailable" });
      })
      .finally(() => {
        if (purchaseNonce.current === nonce) setPurchaseLoading(false);
      });
  }, []);

  const closePurchase = useCallback(() => {
    if (pending === "buy") return;
    purchaseNonce.current += 1;
    setPurchaseTarget(null);
    setPurchase(null);
  }, [pending]);

  const confirmPurchase = useCallback(async () => {
    if (purchase?.status !== "ready") return;
    const quote = purchase.quote;
    setPending("buy");
    setFeedback(null);
    try {
      const result = await invoke<ModuleDealActionView>("buy_module_listing", {
        listingId: quote.listingId,
        tokenId: quote.module.tokenId,
        seller: quote.seller,
        priceWei: quote.priceWei,
      });
      setFeedback(`PURCHASE CONFIRMED · DEAL ${result.deal.dealId}`);
      setPurchaseTarget(null);
      setPurchase(null);
      refresh();
    } catch {
      setFeedback("PURCHASE NOT CONFIRMED · ACCEPTED STATE REFRESHED");
      setPurchaseTarget(null);
      setPurchase(null);
      refresh();
    } finally {
      setPending(null);
    }
  }, [purchase, refresh]);

  const mutateDeal = useCallback(async (deal: ModuleDealView, command: string, label: string) => {
    setPending(`${command}:${deal.dealId}`);
    setFeedback(null);
    try {
      const result = await invoke<ModuleDealActionView>(command, { dealId: deal.dealId });
      setFeedback(`${label} · DEAL ${result.deal.dealId}`);
    } catch {
      setFeedback(`${label} NOT CONFIRMED · ACCEPTED STATE REFRESHED`);
    } finally {
      setPending(null);
      refresh();
    }
  }, [refresh]);

  const visible = useMemo(
    () => selectListings(catalog?.listings ?? [], filter, search, sort),
    [catalog?.listings, filter, search, sort],
  );
  const filtered = filter !== "ALL" || search.trim() !== "";

  return (
    <div
      aria-busy={loading}
      style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}
    >
      <Panel
        label="MODULE MARKET"
        action={
          <Button
            type="button"
            tone="ghost"
            size="sm"
            className="cm-touch"
            disabled={loading}
            onClick={refresh}
          >
            {loading ? "READING CHAIN" : "REFRESH"}
          </Button>
        }
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "var(--space-5)",
            padding: "var(--space-6)",
          }}
        >
          <Input
            type="search"
            value={search}
            onChange={(event) => setSearch((event.currentTarget as HTMLInputElement).value)}
            aria-label="Search module listings"
            placeholder="SEARCH NAME, MODULE, TOKEN, SELLER"
            disabled={catalog?.status !== "available"}
          />

          <div
            role="tablist"
            aria-label="Module slot filter"
            style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-3)" }}
          >
            {FILTERS.map((option) => (
              <button
                key={option}
                type="button"
                role="tab"
                aria-selected={filter === option}
                className="cm-touch"
                disabled={catalog?.status !== "available"}
                onClick={() => setFilter(option)}
                style={{
                  minWidth: 0,
                  padding: "var(--space-3) var(--space-4)",
                  border: filter === option ? "var(--border-loud-style)" : "var(--border-hairline-style)",
                  background: "transparent",
                  color: filter === option ? "var(--text-primary)" : "var(--text-muted)",
                  fontFamily: "var(--type-label-family)",
                  fontSize: "var(--text-2xs)",
                  letterSpacing: "var(--tracking-widest)",
                  cursor: catalog?.status === "available" ? "pointer" : "not-allowed",
                }}
              >
                {option}
              </button>
            ))}
          </div>

          <Select
            value={sort}
            options={SORT_OPTIONS}
            onChange={(event) => setSort((event.currentTarget as HTMLSelectElement).value as MarketSort)}
            aria-label="Sort module listings"
            disabled={catalog?.status !== "available"}
          />

          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-wider)",
              color: "var(--text-muted)",
            }}
          >
            {catalog?.verifiedBlock
              ? `ACCEPTED CATALOG BLOCK ${catalog.verifiedBlock}`
              : "NO ACCEPTED CATALOG BLOCK"}
          </span>
        </div>
      </Panel>

      <div aria-live="polite">
        {loading ? (
          <MarketNotice
            title="LOADING ACTIVE LISTINGS"
            body="Reading listing state, token existence, canonical metadata, and seller standing."
          />
        ) : catalog?.status === "deployment_unavailable" ? (
          <MarketNotice
            title="CANONICAL MARKET UNAVAILABLE"
            body="This release has no reviewed CabalMeshModules and Marketplace deployment pair. Legacy vouchers are not shown as modules."
          />
        ) : catalog?.status === "rpc_failure" ? (
          <MarketNotice
            alert
            title={browserOnline ? "CHAIN READ FAILED" : "OFFLINE"}
            body={
              browserOnline
                ? "The reviewed marketplace could not be read. Retry without trusting an older catalog."
                : "No network is available. MARKET does not present cached listings as currently active."
            }
          />
        ) : catalog?.status === "available" ? (
          <>
            {catalog.staleListings > 0 ? (
              <MarketNotice
                alert
                title="STALE LISTINGS OMITTED"
                body={`${catalog.staleListings} active mapping entr${catalog.staleListings === 1 ? "y was" : "ies were"} hidden because current token ownership, eligibility, or approval no longer backed a purchase.`}
              />
            ) : null}
            {catalog.malformedMetadata > 0 ? (
              <MarketNotice
                alert
                title="MALFORMED METADATA OMITTED"
                body={`${catalog.malformedMetadata} listing${catalog.malformedMetadata === 1 ? "" : "s"} could not be rendered from the canonical module schema.`}
              />
            ) : null}

            {visible.length === 0 ? (
              <MarketNotice
                title={filtered ? "NO MATCHING MODULES" : "NO ACTIVE MODULE LISTINGS"}
                body={
                  filtered
                    ? "No currently buyable canonical module matches this filter and search."
                    : "The accepted catalog contains no currently buyable canonical modules."
                }
                action={
                  filtered ? (
                    <Button
                      type="button"
                      tone="secondary"
                      size="sm"
                      onClick={() => {
                        setFilter("ALL");
                        setSearch("");
                      }}
                    >
                      CLEAR FILTERS
                    </Button>
                  ) : undefined
                }
              />
            ) : (
              <div
                aria-label={`${visible.length} active module listings`}
                style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}
              >
                {visible.map((listing) => (
                  <ListingCard
                    key={`${listing.module.contract}:${listing.module.tokenId}:${listing.listingId}`}
                    listing={listing}
                    disabled={pending !== null}
                    onBuy={() => openPurchase(listing)}
                  />
                ))}
              </div>
            )}
          </>
        ) : null}
      </div>

      {feedback ? (
        <div role="status" style={{ fontSize: "var(--text-sm)", color: "var(--text-primary)" }}>
          {feedback}
        </div>
      ) : null}

      <Panel
        label="MY MODULE DEALS"
        action={
          deals?.verifiedBlock ? (
            <span style={{ fontFamily: "var(--type-data-family)", fontSize: "var(--text-2xs)", color: "var(--text-muted)" }}>
              BLOCK {deals.verifiedBlock}
            </span>
          ) : undefined
        }
      >
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)", padding: "var(--space-6)" }}>
          {dealsLoading ? (
            <DealNotice>READING ACCEPTED DEAL STATE.</DealNotice>
          ) : deals?.status === "deployment_unavailable" ? (
            <DealNotice>NO REVIEWED MODULE MARKET DEPLOYMENT FOR THIS RELEASE.</DealNotice>
          ) : deals?.status === "chain_unavailable" ? (
            <DealNotice>DEAL STATE UNAVAILABLE. NO SETTLEMENT ACTIONS ARE OFFERED.</DealNotice>
          ) : deals?.deals.length === 0 ? (
            <DealNotice>NO MODULE DEALS INVOLVE THIS WALLET.</DealNotice>
          ) : (
            deals?.deals.map((deal) => (
              <DealCard
                key={deal.dealId}
                deal={deal}
                pending={pending}
                onRelease={() => void mutateDeal(deal, "release_module_deal", "RELEASE CONFIRMED")}
                onRequestRefund={() => void mutateDeal(deal, "request_module_refund", "CANCELLATION REQUEST CONFIRMED")}
                onRefund={() => void mutateDeal(deal, "refund_module_deal", "REFUND CONFIRMED")}
              />
            ))
          )}
        </div>
      </Panel>

      <Panel label="ESCROW MODEL">
        <p style={{ margin: 0, padding: "var(--space-6)", fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>
          AVAX and the module move into escrow atomically when purchased. The settlement window governs cancellation,
          not delivery of an off-chain item.
        </p>
      </Panel>

      <PurchaseDialog
        target={purchaseTarget}
        state={purchase}
        loading={purchaseLoading}
        pending={pending === "buy"}
        onClose={closePurchase}
        onConfirm={() => void confirmPurchase()}
      />
    </div>
  );
}

function ListingCard({
  listing,
  disabled,
  onBuy,
}: {
  listing: ModuleMarketListing;
  disabled: boolean;
  onBuy: () => void;
}) {
  const titleId = `market-listing-${listing.listingId}`;
  return (
    <article aria-labelledby={titleId}>
      <Panel
        label={`${listing.module.slot} MODULE`}
        action={<Badge tone={rarityTone(listing.module.rarity)} size="sm">{listing.module.rarity}</Badge>}
      >
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "minmax(0, 1fr) auto",
            gap: "var(--space-5)",
            padding: "var(--space-6)",
            alignItems: "start",
          }}
        >
          <div style={{ minWidth: 0 }}>
            <h2
              id={titleId}
              style={{
                margin: 0,
                fontFamily: "var(--type-heading-family)",
                fontSize: "var(--text-sm)",
                letterSpacing: "var(--type-heading-tracking)",
                color: "var(--text-primary)",
                overflowWrap: "anywhere",
              }}
            >
              {listing.module.displayName}
            </h2>
            <div style={{ marginTop: "var(--space-3)", fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>
              {listing.module.effect}
            </div>
          </div>

          <div
            aria-hidden="true"
            style={{
              width: "var(--space-10)",
              aspectRatio: "1",
              border: "var(--border-default-style)",
              backgroundImage: "var(--texture-grid)",
              backgroundSize: "var(--texture-grid-size)",
            }}
          />
        </div>

        <dl
          style={{
            margin: 0,
            display: "grid",
            gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)",
            gap: "var(--space-4)",
            padding: "var(--space-5) var(--space-6)",
            borderTop: "var(--border-hairline-style)",
          }}
        >
          <Fact label="TOKEN" value={`#${listing.module.tokenId}`} />
          <Fact label="MODULE ID" value={shortHash(listing.module.moduleId)} />
          <Fact label="SELLER" value={shortAddress(listing.seller)} />
          <Fact label="STANDING" value={standingLabel(listing.standing)} />
        </dl>

        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            justifyContent: "space-between",
            alignItems: "baseline",
            gap: "var(--space-4)",
            padding: "var(--space-5) var(--space-6)",
            borderTop: "var(--border-hairline-style)",
          }}
        >
          <span
            style={{
              fontFamily: "var(--type-data-family)",
              fontSize: "var(--text-lg)",
              color: "var(--text-primary)",
              overflowWrap: "anywhere",
            }}
          >
            {listing.priceAvax} AVAX
          </span>
          <div style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: "var(--space-4)" }}>
            <span
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-wider)",
                color: "var(--text-muted)",
              }}
            >
              LISTING {listing.listingId}
            </span>
            <Button type="button" tone="primary" size="sm" disabled={disabled} onClick={onBuy}>
              BUY
            </Button>
          </div>
        </div>
      </Panel>
    </article>
  );
}

function PurchaseDialog({
  target,
  state,
  loading,
  pending,
  onClose,
  onConfirm,
}: {
  target: ModuleMarketListing | null;
  state: ModulePurchaseStateView | null;
  loading: boolean;
  pending: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const quote = state?.status === "ready" || state?.status === "insufficient_funds" ? state.quote : null;
  return (
    <ModalDialog
      open={target !== null}
      title="CONFIRM MODULE PURCHASE"
      onClose={onClose}
      footer={
        <div style={{ display: "flex", justifyContent: "flex-end", flexWrap: "wrap", gap: "var(--space-4)" }}>
          <Button type="button" tone="ghost" disabled={pending} onClick={onClose}>CANCEL</Button>
          {state?.status === "ready" ? (
            <Button type="button" tone="primary" disabled={pending} onClick={onConfirm}>
              {pending ? "PURCHASE PENDING" : `PAY ${state.quote.priceAvax} AVAX`}
            </Button>
          ) : null}
        </div>
      }
    >
      <div aria-busy={loading || pending} style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
        {loading ? (
          <DealNotice>RE-READING LISTING, OWNERSHIP, BALANCE, AND NETWORK FEE.</DealNotice>
        ) : state?.status === "deployment_unavailable" ? (
          <DealNotice>THIS RELEASE HAS NO REVIEWED CANONICAL MODULE MARKET.</DealNotice>
        ) : state?.status === "chain_unavailable" || state === null ? (
          <DealNotice>PURCHASE STATE COULD NOT BE VERIFIED. NO TRANSACTION IS OFFERED.</DealNotice>
        ) : state.status === "inactive" ? (
          <DealNotice>THIS LISTING IS NO LONGER ACTIVE.</DealNotice>
        ) : state.status === "self_purchase" ? (
          <DealNotice>THE CURRENT WALLET IS THE SELLER AND CANNOT BUY ITS OWN LISTING.</DealNotice>
        ) : state.status === "stale_listing" ? (
          <DealNotice>CURRENT OWNERSHIP OR ELIGIBILITY NO LONGER BACKS THIS LISTING.</DealNotice>
        ) : (
          <>
            {quote ? <PurchaseQuoteFacts quote={quote} /> : null}
            {state.status === "insufficient_funds" ? (
              <div role="alert" style={{ fontSize: "var(--text-sm)", color: "var(--accent-blood-red)" }}>
                INSUFFICIENT FUNDS · SHORT {state.shortfallAvax} AVAX ({state.shortfallWei} wei).
              </div>
            ) : null}
            <Panel label="ESCROW TERMS">
              <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)", padding: "var(--space-5)" }}>
                <span style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>
                  Buying atomically locks the exact AVAX price and moves this on-chain module into the marketplace.
                </span>
                <span style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>
                  The three-day window is only for mutually agreed cancellation. There is no off-chain delivery to inspect and the seller owes no later delivery step. Release pays the seller and transfers the module to you; cancellation requires your request and the seller&apos;s refund transaction.
                </span>
              </div>
            </Panel>
          </>
        )}
      </div>
    </ModalDialog>
  );
}

function PurchaseQuoteFacts({ quote }: { quote: Extract<ModulePurchaseStateView, { status: "ready" }> ["quote"] }) {
  return (
    <dl style={{ margin: 0, display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)", gap: "var(--space-4)" }}>
      <Fact label="MODULE" value={quote.module.displayName} />
      <Fact label="TOKEN" value={`#${quote.module.tokenId}`} />
      <Fact label="COLLECTION" value={quote.module.contract} />
      <Fact label="SELLER" value={quote.seller} />
      <Fact label="LISTING PRICE" value={`${quote.priceAvax} AVAX · ${quote.priceWei} wei`} />
      <Fact
        label="NETWORK FEE ESTIMATE"
        value={quote.estimatedNetworkFeeAvax && quote.estimatedNetworkFeeWei
          ? `${quote.estimatedNetworkFeeAvax} AVAX · ${quote.estimatedNetworkFeeWei} wei`
          : "UNAVAILABLE WHILE BALANCE IS BELOW PRICE"}
      />
      <Fact
        label="ESTIMATED TOTAL"
        value={quote.estimatedTotalAvax && quote.estimatedTotalWei
          ? `${quote.estimatedTotalAvax} AVAX · ${quote.estimatedTotalWei} wei`
          : `AT LEAST ${quote.priceAvax} AVAX`}
      />
      <Fact label="ACCEPTED BLOCK" value={quote.verifiedBlock} />
    </dl>
  );
}

function DealCard({
  deal,
  pending,
  onRelease,
  onRequestRefund,
  onRefund,
}: {
  deal: ModuleDealView;
  pending: string | null;
  onRelease: () => void;
  onRequestRefund: () => void;
  onRefund: () => void;
}) {
  const busy = pending !== null;
  const deadline = formatDeadline(deal.autoReleaseAt);
  const settlement = deal.status === "released"
    ? `RELEASED · MODULE OWNER ${shortAddress(deal.currentOwner)}`
    : deal.status === "refunded"
      ? `REFUNDED · MODULE OWNER ${shortAddress(deal.currentOwner)}`
      : deal.releaseAuthority === "buyer_now"
        ? `BUYER MAY RELEASE NOW · ANYONE MAY RELEASE AFTER ${deadline}`
        : deal.releaseAuthority === "anyone_now"
          ? `AUTO-RELEASE DEADLINE PASSED · ANYONE MAY RELEASE NOW`
          : `BUYER MAY RELEASE UNTIL ${deadline} · ANYONE MAY RELEASE AFTER`;
  return (
    <article aria-label={`Deal ${deal.dealId}`} style={{ border: "var(--border-hairline-style)", padding: "var(--space-5)" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
        <div style={{ display: "flex", justifyContent: "space-between", flexWrap: "wrap", gap: "var(--space-3)" }}>
          <strong style={{ fontFamily: "var(--type-heading-family)", color: "var(--text-primary)" }}>
            {deal.module.displayName}
          </strong>
          <Badge tone={deal.status === "active" ? "info" : deal.status === "released" ? "success" : "quiet"} size="sm">
            {deal.status.toUpperCase()} · {deal.role.toUpperCase()}
          </Badge>
        </div>
        <dl style={{ margin: 0, display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)", gap: "var(--space-3)" }}>
          <Fact label="DEAL" value={deal.dealId} />
          <Fact label="TOKEN" value={`#${deal.module.tokenId}`} />
          <Fact label="AMOUNT" value={`${deal.amountAvax} AVAX · ${deal.amountWei} wei`} />
          <Fact label="ACCEPTED BLOCK" value={deal.verifiedBlock} />
          <Fact label="BUYER" value={shortAddress(deal.buyer)} />
          <Fact label="SELLER" value={shortAddress(deal.seller)} />
        </dl>
        <span style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>{settlement}</span>
        {deal.status === "active" && deal.refundRequested ? (
          <span role="status" style={{ fontSize: "var(--text-sm)", color: "var(--text-primary)" }}>
            BUYER REQUESTED CANCELLATION · AWAITING SELLER REFUND. THIS IS NOT A COMPLETED REFUND; THE BUYER MAY STILL RELEASE.
          </span>
        ) : null}
        {deal.status === "active" ? (
          <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-4)" }}>
            {deal.canRelease ? (
              <Button type="button" tone="primary" size="sm" disabled={busy} onClick={onRelease}>
                {pending === `release_module_deal:${deal.dealId}` ? "RELEASE PENDING" : "RELEASE DEAL"}
              </Button>
            ) : null}
            {deal.canRequestRefund ? (
              <Button type="button" tone="secondary" size="sm" disabled={busy} onClick={onRequestRefund}>
                {pending === `request_module_refund:${deal.dealId}` ? "REQUEST PENDING" : "REQUEST CANCELLATION"}
              </Button>
            ) : null}
            {deal.canRefund ? (
              <Button type="button" tone="secondary" size="sm" disabled={busy} onClick={onRefund}>
                {pending === `refund_module_deal:${deal.dealId}` ? "REFUND PENDING" : "REFUND BY MUTUAL AGREEMENT"}
              </Button>
            ) : null}
          </div>
        ) : null}
      </div>
    </article>
  );
}

function DealNotice({ children }: { children: React.ReactNode }) {
  return <span style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>{children}</span>;
}

function formatDeadline(seconds: string): string {
  const value = Number(seconds);
  if (!Number.isSafeInteger(value) || value <= 0) return "UNKNOWN DEADLINE";
  return new Date(value * 1_000).toISOString().replace(".000Z", "Z");
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ minWidth: 0 }}>
      <dt
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-muted)",
        }}
      >
        {label}
      </dt>
      <dd
        style={{
          margin: "var(--space-2) 0 0",
          fontFamily: "var(--type-data-family)",
          fontSize: "var(--text-xs)",
          color: "var(--text-primary)",
          overflowWrap: "anywhere",
        }}
      >
        {value}
      </dd>
    </div>
  );
}

function MarketNotice({
  title,
  body,
  alert = false,
  action,
}: {
  title: string;
  body: string;
  alert?: boolean;
  action?: React.ReactNode;
}) {
  return (
    <Panel label={title}>
      <div
        role={alert ? "alert" : "status"}
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          gap: "var(--space-5)",
          padding: "var(--space-6)",
        }}
      >
        <span style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>{body}</span>
        {action}
      </div>
    </Panel>
  );
}

export function selectListings(
  listings: readonly ModuleMarketListing[],
  filter: MarketFilter,
  search: string,
  sort: MarketSort,
): ModuleMarketListing[] {
  const query = search.trim().toLowerCase();
  return listings
    .filter((listing) => filter === "ALL" || listing.module.slot === filter)
    .filter((listing) => {
      if (query === "") return true;
      return [
        listing.module.displayName,
        listing.module.moduleId,
        listing.module.tokenId,
        listing.seller,
      ].some((value) => value.toLowerCase().includes(query));
    })
    .slice()
    .sort((left, right) => compareListings(left, right, sort));
}

function compareListings(left: ModuleMarketListing, right: ModuleMarketListing, sort: MarketSort): number {
  let order = 0;
  if (sort === "PRICE_ASC" || sort === "PRICE_DESC") {
    order = compareBigInt(left.priceWei, right.priceWei);
    if (sort === "PRICE_DESC") order *= -1;
  } else if (sort === "RARITY_DESC") {
    order = RARITY_RANK[right.module.rarity] - RARITY_RANK[left.module.rarity];
  } else {
    order = compareText(left.module.displayName, right.module.displayName);
  }

  return order
    || compareText(left.module.contract, right.module.contract)
    || compareBigInt(left.module.tokenId, right.module.tokenId)
    || compareBigInt(left.listingId, right.listingId);
}

function compareBigInt(left: string, right: string): number {
  const a = BigInt(left);
  const b = BigInt(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

function compareText(left: string, right: string): number {
  const a = left.toUpperCase();
  const b = right.toUpperCase();
  return a < b ? -1 : a > b ? 1 : 0;
}

function standingLabel(standing: SellerStandingView): string {
  return standing.status === "verified"
    ? `VERIFIED ${standing.value} · BLOCK ${standing.verifiedBlock}`
    : `UNKNOWN · ${unknownStandingLabel(standing.reason)}`;
}

function unknownStandingLabel(reason: SellerStandingUnknownReason): string {
  switch (reason) {
    case "unconfigured":
      return "SOURCE UNCONFIGURED";
    case "unavailable":
      return "QUORUM UNAVAILABLE";
    case "identity_mismatch":
      return "SOURCE MISMATCH";
    case "stale":
      return "STALE EVIDENCE";
    case "unfinalized":
      return "NOT FINAL";
    case "conflicting_providers":
      return "PROVIDERS CONFLICT";
    case "malformed":
      return "MALFORMED EVIDENCE";
  }
}

function rarityTone(rarity: ModuleMarketListing["module"]["rarity"]): "quiet" | "info" | "loud" | "success" {
  switch (rarity) {
    case "COMMON":
      return "quiet";
    case "RARE":
      return "info";
    case "EPIC":
      return "loud";
    case "LEGENDARY":
      return "success";
  }
}

function shortAddress(address: string): string {
  return address.length > 12 ? `${address.slice(0, 6)}…${address.slice(-4)}` : address;
}

function shortHash(hash: string): string {
  return hash.length > 18 ? `${hash.slice(0, 10)}…${hash.slice(-6)}` : hash;
}
