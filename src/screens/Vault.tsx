import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, IconButton, Panel } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import type { VaultTab } from "../shell/screen";
import type {
  ModuleInventory,
  ModuleListingActionView,
  ModuleListingIneligibleReason,
  ModuleListingStateView,
  ModuleView,
  NodeLoadout,
  VaultRow,
} from "../types/bindings";

const TABS: VaultTab[] = ["ASSETS", "MODULES", "IDENTITIES", "KEYS"];

const ROW_COMMAND: Record<Exclude<VaultTab, "MODULES">, string> = {
  ASSETS: "vault_assets",
  IDENTITIES: "vault_identities",
  KEYS: "vault_keys",
};

/**
 * Assets, authentic modules, identities and key metadata.
 *
 * MODULES is deliberately a different data path from ordinary vault rows. It
 * is rebuilt from the configured canonical ERC-721 collection every time and
 * never from a pending receipt, marketplace description, legacy voucher, or
 * optimistic cache. A failed refresh therefore clears the inventory instead
 * of leaving stale ownership on screen as if the chain still confirmed it.
 *
 * **The total is masked by default and only fetched on reveal.** Sending the
 * value and hiding it in CSS would put the balance in the DOM of a screen the
 * user asked not to show it on — masking is presentation, so the *value* has to
 * be absent, not merely covered.
 *
 * The KEYS tab never renders key material. It describes what is held and where;
 * the values stay in the encrypted vault. That is the promise the screen's own
 * copy makes, and the command keeps it too.
 */
export function Vault({ tab, onTabChange }: { tab: VaultTab; onTabChange: (tab: VaultTab) => void }) {
  const [rows, setRows] = useState<VaultRow[] | null>(null);
  const [modules, setModules] = useState<ModuleInventory | null>(null);
  const [loadout, setLoadout] = useState<NodeLoadout | null>(null);
  const [moduleError, setModuleError] = useState(false);
  const [moduleBusy, setModuleBusy] = useState(false);
  const [moduleActionBusy, setModuleActionBusy] = useState(false);
  const [moduleActionError, setModuleActionError] = useState(false);
  const [moduleRefresh, setModuleRefresh] = useState(0);
  const [selectedModule, setSelectedModule] = useState<ModuleView | null>(null);
  const [revealed, setRevealed] = useState(false);

  const refreshModules = useCallback(() => setModuleRefresh((value) => value + 1), []);

  useEffect(() => {
    if (tab === "MODULES") return;

    let cancelled = false;
    setRows(null);
    invoke<VaultRow[]>(ROW_COMMAND[tab])
      .then((next) => {
        if (!cancelled) setRows(next);
      })
      .catch(() => {
        if (!cancelled) setRows([]);
      });
    return () => {
      cancelled = true;
    };
  }, [tab]);

  useEffect(() => {
    if (tab !== "MODULES") return;

    let cancelled = false;
    setModuleBusy(true);
    setModuleError(false);
    invoke<ModuleInventory>("vault_modules")
      .then((next) => {
        if (cancelled) return;
        setModules(next);
        setSelectedModule((selected) =>
          selected && next.modules.some((candidate) => moduleKey(candidate) === moduleKey(selected))
            ? next.modules.find((candidate) => moduleKey(candidate) === moduleKey(selected)) ?? null
            : null,
        );
      })
      .catch(() => {
        if (cancelled) return;
        // The previous response is not evidence of current ownership after a
        // failed refresh. Blank it rather than presenting a stale assertion.
        setModules(null);
        setSelectedModule(null);
        setModuleError(true);
      })
      .finally(() => {
        if (!cancelled) setModuleBusy(false);
      });

    return () => {
      cancelled = true;
    };
  }, [moduleRefresh, tab]);

  useEffect(() => {
    if (tab !== "MODULES") return;

    let cancelled = false;
    setLoadout(null);
    invoke<NodeLoadout>("module_loadout")
      .then((next) => {
        if (!cancelled) setLoadout(next);
      })
      .catch(() => {
        if (!cancelled) setLoadout(null);
      });

    return () => {
      cancelled = true;
    };
  }, [moduleRefresh, tab]);

  const mutateLoadout = useCallback(async (command: "equip_module" | "unequip_module", module: ModuleView) => {
    setModuleActionBusy(true);
    setModuleActionError(false);
    try {
      const next = await invoke<NodeLoadout>(command, { tokenId: module.tokenId });
      setLoadout(next);
    } catch {
      setModuleActionError(true);
      setModuleRefresh((value) => value + 1);
    } finally {
      setModuleActionBusy(false);
    }
  }, []);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <div
        role="tablist"
        aria-label="Vault section"
        style={{ display: "flex", flexWrap: "wrap", columnGap: "var(--space-7)", rowGap: "var(--space-2)" }}
      >
        {TABS.map((name) => (
          <button
            key={name}
            type="button"
            role="tab"
            aria-selected={tab === name}
            className="cm-touch"
            onClick={() => onTabChange(name)}
            style={{
              background: "none",
              border: "none",
              padding: "var(--space-4) 0",
              cursor: "pointer",
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: tab === name ? "var(--text-primary)" : "var(--text-muted)",
              borderBottom:
                tab === name
                  ? "var(--border-width-thick) solid var(--border-loud)"
                  : "var(--border-width-thick) solid transparent",
            }}
          >
            {name}
          </button>
        ))}
      </div>

      {tab === "ASSETS" ? (
        <Panel label="TOTAL VALUE (PRIVATE)">
          <div
            className="cm-row"
            style={{
              padding: "var(--space-6)",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: "var(--space-5)",
            }}
          >
            <span
              style={{
                fontFamily: "var(--type-data-family)",
                fontSize: "var(--text-lg)",
                letterSpacing: "var(--type-data-tracking)",
                color: "var(--text-primary)",
              }}
            >
              {/* Not fetched unless revealed — see the note above. */}
              {revealed ? "—" : "✱✱✱✱✱"}
            </span>
            <IconButton
              size="md"
              tone="outline"
              className="cm-touch"
              aria-label={revealed ? "Hide total value" : "Reveal total value"}
              aria-pressed={revealed}
              onClick={() => setRevealed((previous) => !previous)}
            >
              {revealed ? "×" : "◎"}
            </IconButton>
          </div>
        </Panel>
      ) : null}

      {tab === "MODULES" ? (
        <>
          <NodeLoadoutPanel loadout={loadout} actionFailed={moduleActionError} />
          <ModuleInventoryPanel
            inventory={modules}
            loadout={loadout}
            busy={moduleBusy || moduleActionBusy}
            failed={moduleError}
            onRefresh={refreshModules}
            onSelect={setSelectedModule}
          />
        </>
      ) : (
        <Panel label={tab}>
          {rows === null ? null : rows.length === 0 ? (
            <Empty tab={tab} />
          ) : (
            rows.map((row) => <Row key={`${row.tag}-${row.name}`} row={row} />)
          )}
        </Panel>
      )}

      <ModuleDetails
        module={selectedModule}
        loadout={loadout}
        busy={moduleActionBusy}
        onMutate={mutateLoadout}
        onClose={() => setSelectedModule(null)}
      />
    </div>
  );
}

function NodeLoadoutPanel({ loadout, actionFailed }: { loadout: NodeLoadout | null; actionFailed: boolean }) {
  const state = loadoutStatus(loadout);

  return (
    <Panel
      label="NODE LOADOUT"
      action={<Badge tone={state.tone} size="sm">{state.label}</Badge>}
    >
      {actionFailed ? (
        <ModuleNotice
          title="LOADOUT CHANGE NOT CONFIRMED"
          body="Accepted chain state did not confirm that change. The loadout was refreshed without optimistic state."
        />
      ) : null}

      {loadout === null ? (
        <ModuleNotice title="VERIFYING NODE LOADOUT" body="Reading ownership and slot bindings from one chain head." />
      ) : loadout.status === "collection_unavailable" ? (
        <ModuleNotice
          title="CANONICAL COLLECTION UNAVAILABLE"
          body="No reviewed module collection is configured, so this node has no authentic loadout."
        />
      ) : loadout.status === "chain_unavailable" ? (
        <ModuleNotice
          title="CHAIN AND CACHE UNAVAILABLE"
          body="No currently verified loadout or matching prior snapshot can be shown."
        />
      ) : (
        <div>
          <div style={{ padding: "var(--space-4) var(--space-6)", borderTop: "var(--border-hairline-style)" }}>
            <div
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                color: "var(--text-muted)",
              }}
            >
              NODE OPERATOR
            </div>
            <div
              style={{
                marginTop: "var(--space-2)",
                fontFamily: "var(--type-data-family)",
                fontSize: "var(--text-xs)",
                color: "var(--text-primary)",
                overflowWrap: "anywhere",
              }}
            >
              {loadout.operator}
            </div>
          </div>

          {loadout.slots.map((slot) => (
            <div
              key={slot.slot}
              className="cm-row"
              style={{
                display: "grid",
                gridTemplateColumns: "minmax(4.5rem, 0.4fr) minmax(0, 1fr)",
                gap: "var(--space-5)",
                alignItems: "center",
              }}
            >
              <span
                style={{
                  fontFamily: "var(--type-label-family)",
                  fontSize: "var(--text-xs)",
                  letterSpacing: "var(--tracking-widest)",
                  color: "var(--text-secondary)",
                }}
              >
                {slot.slot}
              </span>
              <span style={{ minWidth: 0 }}>
                <span
                  style={{
                    display: "block",
                    fontFamily: "var(--type-heading-family)",
                    fontSize: "var(--text-sm)",
                    color: slot.module ? "var(--text-primary)" : "var(--text-muted)",
                    overflowWrap: "anywhere",
                  }}
                >
                  {slot.module?.displayName ?? "EMPTY"}
                </span>
                <span
                  style={{
                    display: "block",
                    marginTop: "var(--space-1)",
                    fontFamily: "var(--type-label-family)",
                    fontSize: "var(--text-2xs)",
                    letterSpacing: "var(--tracking-wider)",
                    color: "var(--text-muted)",
                  }}
                >
                  {slot.activeEffect ?? "NO ACTIVE VERIFIED EFFECT"}
                </span>
              </span>
            </div>
          ))}

          <div
            style={{
              padding: "var(--space-4) var(--space-6)",
              borderTop: "var(--border-hairline-style)",
              fontSize: "var(--text-sm)",
              color: "var(--text-muted)",
            }}
          >
            {loadout.status === "cached"
              ? `Cached from accepted block ${loadout.verifiedBlock ?? "unknown"}. Display only — not effect eligibility proof.`
              : `Verified at accepted block ${loadout.verifiedBlock ?? "unknown"}. Runtime effects remain inactive until their verifier is implemented.`}
            {loadout.mutationTxHash ? ` Confirmed transaction ${loadout.mutationTxHash}.` : ""}
          </div>
        </div>
      )}
    </Panel>
  );
}

function ModuleInventoryPanel({
  inventory,
  loadout,
  busy,
  failed,
  onRefresh,
  onSelect,
}: {
  inventory: ModuleInventory | null;
  loadout: NodeLoadout | null;
  busy: boolean;
  failed: boolean;
  onRefresh: () => void;
  onSelect: (module: ModuleView) => void;
}) {
  return (
    <Panel
      label="MODULES"
      action={
        <Button
          type="button"
          tone="ghost"
          size="sm"
          className="cm-touch"
          disabled={busy}
          aria-label="Refresh confirmed module ownership"
          onClick={onRefresh}
        >
          {busy ? "READING CHAIN" : "REFRESH"}
        </Button>
      }
    >
      {failed ? (
        <ModuleNotice
          title="OWNERSHIP UNCONFIRMED"
          body="The canonical chain read failed. No cached modules are shown. Retry when the network is reachable."
        />
      ) : inventory === null ? (
        <ModuleNotice title="READING CANONICAL OWNERSHIP" body="Only confirmed tokens will appear here." />
      ) : inventory.status === "unavailable" ? (
        <ModuleNotice
          title="CANONICAL COLLECTION UNAVAILABLE"
          body="This network has no reviewed CabalMeshModules deployment. Legacy vouchers are never treated as authentic modules."
        />
      ) : inventory.modules.length === 0 ? (
        <ModuleNotice
          title="NO CONFIRMED MODULES"
          body="Current on-chain ownership is empty. Pending, failed, replaced, or reorganized mints are not holdings."
        />
      ) : (
        inventory.modules.map((module) => (
          <ModuleRow
            key={moduleKey(module)}
            module={module}
            equipped={isEquipped(loadout, module)}
            onSelect={() => onSelect(module)}
          />
        ))
      )}
    </Panel>
  );
}

function ModuleRow({ module, equipped, onSelect }: { module: ModuleView; equipped: boolean; onSelect: () => void }) {
  return (
    <button
      type="button"
      className="cm-touch cm-row"
      onClick={onSelect}
      aria-label={`Inspect ${module.displayName}, token ${module.tokenId}`}
      style={{
        width: "100%",
        display: "flex",
        alignItems: "center",
        gap: "var(--space-5)",
        padding: "var(--space-5) var(--space-6)",
        border: "none",
        borderTop: "var(--border-hairline-style)",
        background: "none",
        color: "inherit",
        cursor: "pointer",
        textAlign: "left",
      }}
    >
      <span
        aria-hidden="true"
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-wider)",
          color: "var(--text-muted)",
          border: "var(--border-hairline-style)",
          padding: "var(--space-2) var(--space-3)",
        }}
      >
        {module.assetClass === "STANDING_BADGE" ? "BADGE" : module.slot}
      </span>

      <div style={{ minWidth: 0, flex: 1, display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
        <span
          style={{
            fontFamily: "var(--type-heading-family)",
            fontSize: "var(--text-sm)",
            letterSpacing: "var(--type-heading-tracking)",
            color: "var(--text-primary)",
            overflowWrap: "anywhere",
          }}
        >
          {module.displayName}
        </span>
        <span
          style={{
            fontFamily: "var(--type-label-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--tracking-widest)",
            color: module.revoked ? "var(--accent-blood-red)" : "var(--text-muted)",
            overflowWrap: "anywhere",
          }}
        >
          {module.effect}
        </span>
      </div>

      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: "var(--space-2)" }}>
        {equipped ? <Badge tone="info" size="sm">EQUIPPED</Badge> : null}
        {module.soulbound ? <Badge tone="alert" size="sm">SOULBOUND</Badge> : null}
        <span
          style={{
            fontFamily: "var(--type-data-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--type-data-tracking)",
            color: "var(--text-secondary)",
          }}
        >
          #{module.tokenId}
        </span>
      </div>
    </button>
  );
}

function ModuleDetails({
  module,
  loadout,
  busy,
  onMutate,
  onClose,
}: {
  module: ModuleView | null;
  loadout: NodeLoadout | null;
  busy: boolean;
  onMutate: (command: "equip_module" | "unequip_module", module: ModuleView) => void;
  onClose: () => void;
}) {
  const [listing, setListing] = useState<ModuleListingActionView | null>(null);
  const [listingBusy, setListingBusy] = useState<"status" | "approval" | "listing" | "cancel" | null>(null);
  const [listingFailed, setListingFailed] = useState(false);
  const [priceAvax, setPriceAvax] = useState("");
  const action = module ? loadoutAction(loadout, module) : null;
  const selectedKey = module ? moduleKey(module) : null;

  useEffect(() => {
    if (!module || !selectedKey) {
      setListing(null);
      setListingBusy(null);
      setListingFailed(false);
      setPriceAvax("");
      return;
    }

    let cancelled = false;
    setListing(null);
    setListingBusy("status");
    setListingFailed(false);
    setPriceAvax("");
    invoke<ModuleListingActionView>("module_listing_status", { tokenId: module.tokenId })
      .then((next) => {
        if (!cancelled) setListing(next);
      })
      .catch(() => {
        if (cancelled) return;
        setListing({ state: { status: "chain_unavailable" }, operation: "none", txHash: null });
        setListingFailed(true);
      })
      .finally(() => {
        if (!cancelled) setListingBusy(null);
      });
    return () => {
      cancelled = true;
    };
  }, [loadout?.mutationTxHash, selectedKey]);

  const refreshListingAfterFailure = useCallback(async (tokenId: string) => {
    try {
      const current = await invoke<ModuleListingActionView>("module_listing_status", { tokenId });
      setListing(current);
    } catch {
      setListing({ state: { status: "chain_unavailable" }, operation: "none", txHash: null });
    }
  }, []);

  const mutateListing = useCallback(async (
    command: "approve_module_listing" | "create_module_listing" | "cancel_module_listing",
    pending: "approval" | "listing" | "cancel",
    args: Record<string, string>,
  ) => {
    if (!module || listingBusy !== null) return;
    setListingBusy(pending);
    setListingFailed(false);
    try {
      const next = await invoke<ModuleListingActionView>(command, args);
      setListing(next);
    } catch {
      // Never retain a pre-submit state as evidence after a rejection,
      // replacement, timeout, reorg, or revert. Re-read accepted state.
      setListingFailed(true);
      await refreshListingAfterFailure(module.tokenId);
    } finally {
      setListingBusy(null);
    }
  }, [listingBusy, module, refreshListingAfterFailure]);

  const listingBlocksEquip =
    listing === null ||
    listing.state.status === "chain_unavailable" ||
    listing.state.status === "listed" ||
    listing.state.status === "stale_listing" ||
    listing.state.status === "deal_rules_active";
  const loadoutDisabled = busy || Boolean(action?.disabled) ||
    (action?.command === "equip_module" && listingBlocksEquip);
  const loadoutLabel = action?.command === "equip_module"
    ? listing === null
      ? "VERIFY MARKET STATE"
      : listing.state.status === "chain_unavailable"
        ? "MARKET STATE UNAVAILABLE"
        : listing.state.status === "listed" || listing.state.status === "stale_listing"
          ? "CANCEL LISTING FIRST"
          : listing.state.status === "deal_rules_active"
            ? "DEAL RULES ACTIVE"
            : action.label
    : action?.label;

  return (
    <ModalDialog
      open={module !== null}
      title={module?.displayName ?? "MODULE DETAILS"}
      onClose={listingBusy !== null && listingBusy !== "status" ? () => undefined : onClose}
      footer={
        <div style={{ display: "flex", flexWrap: "wrap", justifyContent: "flex-end", gap: "var(--space-3)" }}>
          {module && action ? (
            <Button
              type="button"
              tone="ghost"
              size="md"
              className="cm-touch"
              disabled={loadoutDisabled || listingBusy !== null}
              onClick={() => onMutate(action.command, module)}
            >
              {busy ? "CONFIRMING CHAIN" : loadoutLabel}
            </Button>
          ) : null}
          <Button
            type="button"
            tone="primary"
            size="md"
            className="cm-touch"
            disabled={listingBusy !== null && listingBusy !== "status"}
            onClick={onClose}
          >
            CLOSE
          </Button>
        </div>
      }
    >
      {module ? (
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-3)" }}>
            <Badge tone="loud" size="sm">{moduleAssetClassLabel(module)}</Badge>
            <Badge tone="info" size="sm">{module.rarity}</Badge>
            {module.soulbound ? <Badge tone="alert" size="sm">SOULBOUND · NON-TRANSFERABLE</Badge> : null}
            {module.revoked ? <Badge tone="alert" size="sm">REVOKED</Badge> : null}
          </div>

          <DetailField label="TOKEN ID" value={`#${module.tokenId}`} />
          <DetailField label="CONTRACT" value={module.contract} />
          <DetailField label="CURRENT OWNER" value={module.owner} />
          <DetailField label="MINT PROVENANCE" value={module.provenanceHash} />
          <DetailField label="MODULE ID" value={module.moduleId} />
          <DetailField label="MINTED BY" value={module.mintedBy} />
          <DetailField label="SLOT" value={module.slot} />
          <DetailField label="RARITY" value={module.rarity} />
          <DetailField label="EFFECT TYPE" value={module.effectType.replace(/_/g, " ")} />
          <DetailField label="EFFECT" value={module.effect} />
          <DetailField
            label="NODE LOADOUT"
            value={
              isEquipped(loadout, module)
                ? loadout?.status === "verified"
                  ? "ON-CHAIN EQUIPPED"
                  : "CACHED EQUIPPED · NOT CURRENT PROOF"
                : "NOT EQUIPPED"
            }
          />
          <DetailField label="ACTIVE VERIFIED EFFECT" value="NONE IN THIS BUILD" />
          <DetailField
            label="EFFECT PARAMETERS"
            value={`PRIMARY ${module.primaryEffectValue} · SECONDARY ${module.secondaryEffectValue}`}
          />
          <DetailField label="ARTWORK URI" value={module.artworkUri} />
          <DetailField label="ARTWORK DIGEST" value={module.artworkDigest} />
          <DetailField label="SCHEMA" value={`CABALMESH V${module.schemaVersion}`} />
          <ModuleListingControls
            listing={listing}
            busy={listingBusy}
            failed={listingFailed}
            priceAvax={priceAvax}
            onPriceChange={setPriceAvax}
            onApprove={() => mutateListing(
              "approve_module_listing",
              "approval",
              { tokenId: module.tokenId },
            )}
            onList={() => mutateListing(
              "create_module_listing",
              "listing",
              { tokenId: module.tokenId, priceAvax },
            )}
            onCancel={(listingId) => mutateListing(
              "cancel_module_listing",
              "cancel",
              { tokenId: module.tokenId, listingId },
            )}
          />
        </div>
      ) : null}
    </ModalDialog>
  );
}

function ModuleListingControls({
  listing,
  busy,
  failed,
  priceAvax,
  onPriceChange,
  onApprove,
  onList,
  onCancel,
}: {
  listing: ModuleListingActionView | null;
  busy: "status" | "approval" | "listing" | "cancel" | null;
  failed: boolean;
  priceAvax: string;
  onPriceChange: (value: string) => void;
  onApprove: () => void;
  onList: () => void;
  onCancel: (listingId: string) => void;
}) {
  const state = listing?.state ?? null;
  const canPrice = state?.status === "approval_required" || state?.status === "ready";
  const validPrice = isExactPositiveAvax(priceAvax);
  const confirmed = listingOperationCopy(listing);

  return (
    <section
      aria-label="Module marketplace listing"
      style={{
        border: "var(--border-hairline-style)",
        padding: "var(--space-5)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
      }}
    >
      <div style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-4)", flexWrap: "wrap" }}>
        <span style={listingLabelStyle}>SELL THIS MODULE</span>
        <Badge tone={state?.status === "listed" ? "success" : "quiet"} size="sm">
          {listingStateLabel(state)}
        </Badge>
      </div>

      {busy === "status" || state === null ? (
        <ListingCopy title="VERIFYING MARKET STATE" body="Reading owner, loadout, approval, and duplicate guard from one accepted chain head." />
      ) : null}

      {failed ? (
        <ListingCopy title="OPERATION NOT CONFIRMED" body="The transaction was rejected, replaced, timed out, reorganized, or reverted. Accepted state was refreshed; no optimistic listing is shown." alert />
      ) : null}

      {confirmed ? <ListingCopy title={confirmed.title} body={confirmed.body} /> : null}

      {state?.status === "deployment_unavailable" ? (
        <ListingCopy title="CANONICAL MARKET UNAVAILABLE" body="This release has no reviewed CabalMeshModules and Marketplace deployment pair. Legacy voucher listings are not accepted." />
      ) : state?.status === "chain_unavailable" ? (
        <ListingCopy title="MARKET STATE UNAVAILABLE" body="Ownership and listing state could not be verified. Selling and equipping are paused until accepted state is readable." alert />
      ) : state?.status === "missing_or_burned" ? (
        <ListingCopy title="TOKEN DOES NOT EXIST" body="The canonical collection no longer reports this token. Burned and nonexistent tokens cannot be listed." alert />
      ) : state?.status === "not_owner" ? (
        <ListingCopy title="NOT CURRENT OWNER" body={`Current owner is ${state.owner}. Only that address can list this token.`} alert />
      ) : state?.status === "equipped" ? (
        <ListingCopy title={`UNEQUIP ${state.slot} FIRST`} body="Listing is disabled while the module is bound to the verified node loadout. Confirm the unequip transaction, then reopen this detail." />
      ) : state?.status === "ineligible" ? (
        <ListingCopy title="MODULE IS NOT LISTABLE" body={listingIneligibleCopy(state.reason)} alert />
      ) : state?.status === "deal_rules_active" ? (
        <ListingCopy title="BUYER HAS PAID" body="The listing cannot be cancelled because the token and payment are now in escrow. Release or mutually agreed refund rules govern the active deal." alert />
      ) : state?.status === "listed" || state?.status === "stale_listing" ? (
        <>
          <ListingCopy
            title={state.status === "listed" ? "ACTIVE LISTING VERIFIED" : "LISTING NO LONGER BUYABLE"}
            body={state.status === "listed"
              ? `Listing #${state.listing.listingId} offers token #${state.listing.tokenId} for exactly ${state.listing.priceAvax} AVAX (${state.listing.priceWei} wei).`
              : "The duplicate guard still references this listing, but current ownership, eligibility, loadout, or approval no longer backs a purchase. Withdraw it before trying again."}
          />
          <Button
            type="button"
            tone="ghost"
            size="md"
            className="cm-touch"
            disabled={busy !== null}
            onClick={() => onCancel(state.listing.listingId)}
          >
            {busy === "cancel" ? "CANCELLATION PENDING" : "CANCEL UNSOLD LISTING"}
          </Button>
        </>
      ) : null}

      {canPrice ? (
        <label style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
          <span style={listingLabelStyle}>EXACT PRICE · AVAX</span>
          <input
            type="text"
            inputMode="decimal"
            autoComplete="off"
            spellCheck={false}
            value={priceAvax}
            disabled={busy !== null}
            aria-invalid={priceAvax.length > 0 && !validPrice}
            placeholder="2.40"
            onChange={(event) => onPriceChange(event.currentTarget.value)}
            style={{
              minHeight: "var(--control-min-height)",
              border: "var(--border-hairline-style)",
              borderColor: priceAvax.length > 0 && !validPrice ? "var(--accent-blood-red)" : undefined,
              background: "var(--surface-sunken)",
              color: "var(--text-primary)",
              fontFamily: "var(--type-data-family)",
              fontSize: "var(--text-base)",
              padding: "var(--space-3) var(--space-4)",
            }}
          />
          <span style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>
            Decimal text only, up to 18 places. The value is never converted through JavaScript floating point.
          </span>
        </label>
      ) : null}

      {state?.status === "approval_required" ? (
        <Button type="button" tone="ghost" size="md" className="cm-touch" disabled={busy !== null} onClick={onApprove}>
          {busy === "approval" ? "APPROVAL PENDING" : "1 · APPROVE MARKETPLACE"}
        </Button>
      ) : state?.status === "ready" ? (
        <>
          <ListingCopy
            title={state.approval === "blanket" ? "BLANKET APPROVAL CONFIRMED" : "TOKEN APPROVAL CONFIRMED"}
            body={state.approval === "blanket"
              ? "The existing operator approval is accepted; no redundant approval transaction will be sent."
              : "Approval is accepted. Listing remains a separate transaction."}
          />
          <Button type="button" tone="primary" size="md" className="cm-touch" disabled={busy !== null || !validPrice} onClick={onList}>
            {busy === "listing" ? "LISTING PENDING" : "2 · LIST FOR EXACT PRICE"}
          </Button>
        </>
      ) : null}
    </section>
  );
}

const listingLabelStyle = {
  fontFamily: "var(--type-label-family)",
  fontSize: "var(--text-2xs)",
  letterSpacing: "var(--tracking-widest)",
  color: "var(--text-secondary)",
} as const;

function ListingCopy({ title, body, alert = false }: { title: string; body: string; alert?: boolean }) {
  return (
    <div role={alert ? "alert" : undefined} style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
      <span style={{ ...listingLabelStyle, color: alert ? "var(--accent-blood-red)" : "var(--text-primary)" }}>{title}</span>
      <span style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)", overflowWrap: "anywhere" }}>{body}</span>
    </div>
  );
}

function listingStateLabel(state: ModuleListingStateView | null): string {
  switch (state?.status) {
    case "approval_required": return "APPROVAL REQUIRED";
    case "ready": return "READY TO LIST";
    case "listed": return "LISTED";
    case "stale_listing": return "STALE LISTING";
    case "equipped": return "EQUIPPED";
    case "deal_rules_active": return "IN DEAL";
    case "deployment_unavailable": return "NO DEPLOYMENT";
    case "chain_unavailable": return "CHAIN UNAVAILABLE";
    case "missing_or_burned": return "MISSING";
    case "not_owner": return "NOT OWNER";
    case "ineligible": return "INELIGIBLE";
    default: return "VERIFYING";
  }
}

function listingIneligibleCopy(reason: ModuleListingIneligibleReason): string {
  switch (reason) {
    case "soulbound": return "Soulbound standing badges are non-transferable and cannot enter the market.";
    case "revoked": return "The issuer revoked this module, so it cannot enter a new official marketplace sale.";
    case "incompatible": return "This token does not match the reviewed CabalMesh module schema, slot, and effect rules.";
    case "marketplace_disabled": return "The canonical collection currently marks this token as marketplace-ineligible.";
  }
}

function listingOperationCopy(action: ModuleListingActionView | null): { title: string; body: string } | null {
  switch (action?.operation) {
    case "approval_confirmed":
      return { title: "APPROVAL CONFIRMED", body: `Accepted chain state confirmed marketplace approval${action.txHash ? ` in ${action.txHash}` : ""}.` };
    case "listing_confirmed":
      return { title: "LISTING CONFIRMED", body: `Accepted chain state confirmed the exact token and price${action.txHash ? ` in ${action.txHash}` : ""}.` };
    case "listing_cancelled":
      return { title: "CANCELLATION CONFIRMED", body: `The active listing is gone from accepted state${action.txHash ? ` after ${action.txHash}` : ""}. This module can be equipped or listed again.` };
    case "deal_rules_active":
      return { title: "DEAL RULES TOOK OVER", body: "A buyer paid before cancellation confirmed. The listing was not cancelled." };
    default:
      return null;
  }
}

export function isExactPositiveAvax(value: string): boolean {
  if (!/^[0-9]+(?:\.[0-9]{1,18})?$/.test(value)) return false;
  return value.replace(".", "").split("").some((digit) => digit !== "0");
}

function DetailField({ label, value }: { label: string; value: string }) {
  return (
    <div className="cm-row" style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
      <span
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-muted)",
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: "var(--type-data-family)",
          fontSize: "var(--text-sm)",
          letterSpacing: "var(--type-data-tracking)",
          color: "var(--text-primary)",
          overflowWrap: "anywhere",
        }}
      >
        {value}
      </span>
    </div>
  );
}

function ModuleNotice({ title, body }: { title: string; body: string }) {
  return (
    <div style={{ padding: "var(--space-9) var(--space-6)", textAlign: "center" }}>
      <div
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-primary)",
        }}
      >
        {title}
      </div>
      <p style={{ margin: "var(--space-4) 0 0", fontSize: "var(--text-base)", color: "var(--text-muted)" }}>
        {body}
      </p>
    </div>
  );
}

function Row({ row }: { row: VaultRow }) {
  return (
    <div
      className="cm-row"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-5)",
        padding: "var(--space-5) var(--space-6)",
        borderTop: "var(--border-hairline-style)",
      }}
    >
      <span
        aria-hidden="true"
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-wider)",
          color: "var(--text-muted)",
          border: "var(--border-hairline-style)",
          padding: "var(--space-2) var(--space-3)",
        }}
      >
        {row.tag}
      </span>

      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
        <span
          style={{
            fontFamily: "var(--type-heading-family)",
            fontSize: "var(--text-sm)",
            letterSpacing: "var(--type-heading-tracking)",
            color: "var(--text-primary)",
          }}
        >
          {row.name}
        </span>
        {row.detail ? (
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: "var(--text-muted)",
            }}
          >
            {row.detail}
          </span>
        ) : null}
      </div>

      <span
        style={{
          fontFamily: "var(--type-data-family)",
          fontSize: "var(--text-sm)",
          letterSpacing: "var(--type-data-tracking)",
          color: "var(--text-secondary)",
        }}
      >
        {row.amount}
      </span>
    </div>
  );
}

function Empty({ tab }: { tab: Exclude<VaultTab, "MODULES"> }) {
  const body =
    tab === "ASSETS"
      ? "Nothing is held. Nothing is stored."
      : tab === "IDENTITIES"
        ? "No identity exists yet."
        : "No key material is held.";

  return (
    <div style={{ padding: "var(--space-9) var(--space-6)", textAlign: "center" }}>
      <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>{body}</span>
    </div>
  );
}

function moduleKey(module: ModuleView): string {
  return `${module.contract}:${module.tokenId}`;
}

function moduleAssetClassLabel(module: ModuleView): string {
  return module.assetClass.replace(/_/g, " ");
}

function loadoutStatus(loadout: NodeLoadout | null): {
  label: string;
  tone: "quiet" | "info" | "alert";
} {
  switch (loadout?.status) {
    case "verified":
      return { label: `VERIFIED · #${loadout.verifiedBlock ?? "?"}`, tone: "info" };
    case "cached":
      return { label: "CACHED · DISPLAY ONLY", tone: "alert" };
    case "chain_unavailable":
      return { label: "CHAIN UNAVAILABLE", tone: "alert" };
    case "collection_unavailable":
      return { label: "COLLECTION UNAVAILABLE", tone: "alert" };
    default:
      return { label: "READING CHAIN", tone: "quiet" };
  }
}

function isEquipped(loadout: NodeLoadout | null, module: ModuleView): boolean {
  return Boolean(
    loadout?.slots.some(
      (slot) => slot.module !== null && moduleKey(slot.module) === moduleKey(module),
    ),
  );
}

type LoadoutAction = {
  command: "equip_module" | "unequip_module";
  label: string;
  disabled: boolean;
};

function loadoutAction(loadout: NodeLoadout | null, module: ModuleView): LoadoutAction | null {
  if (module.assetClass !== "MODULE" || module.revoked || module.slot === "NONE") return null;
  if (loadout?.status !== "verified") {
    return { command: "equip_module", label: "VERIFY CHAIN TO CHANGE", disabled: true };
  }

  const slot = loadout.slots.find((candidate) => candidate.slot === module.slot);
  if (slot?.module && moduleKey(slot.module) === moduleKey(module)) {
    return { command: "unequip_module", label: `UNEQUIP ${module.slot}`, disabled: false };
  }
  if (slot?.module) {
    return { command: "equip_module", label: `UNEQUIP ${module.slot} FIRST`, disabled: true };
  }
  return { command: "equip_module", label: `EQUIP ${module.slot}`, disabled: false };
}
