import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, IconButton, Panel } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import type { VaultTab } from "../shell/screen";
import type { ModuleInventory, ModuleView, VaultRow } from "../types/bindings";

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
  const [moduleError, setModuleError] = useState(false);
  const [moduleBusy, setModuleBusy] = useState(false);
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
        <ModuleInventoryPanel
          inventory={modules}
          busy={moduleBusy}
          failed={moduleError}
          onRefresh={refreshModules}
          onSelect={setSelectedModule}
        />
      ) : (
        <Panel label={tab}>
          {rows === null ? null : rows.length === 0 ? (
            <Empty tab={tab} />
          ) : (
            rows.map((row) => <Row key={`${row.tag}-${row.name}`} row={row} />)
          )}
        </Panel>
      )}

      <ModuleDetails module={selectedModule} onClose={() => setSelectedModule(null)} />
    </div>
  );
}

function ModuleInventoryPanel({
  inventory,
  busy,
  failed,
  onRefresh,
  onSelect,
}: {
  inventory: ModuleInventory | null;
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
          <ModuleRow key={moduleKey(module)} module={module} onSelect={() => onSelect(module)} />
        ))
      )}
    </Panel>
  );
}

function ModuleRow({ module, onSelect }: { module: ModuleView; onSelect: () => void }) {
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

function ModuleDetails({ module, onClose }: { module: ModuleView | null; onClose: () => void }) {
  return (
    <ModalDialog
      open={module !== null}
      title={module?.displayName ?? "MODULE DETAILS"}
      onClose={onClose}
      footer={
        <Button type="button" tone="primary" size="md" className="cm-touch" onClick={onClose}>
          CLOSE
        </Button>
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
            label="EFFECT PARAMETERS"
            value={`PRIMARY ${module.primaryEffectValue} · SECONDARY ${module.secondaryEffectValue}`}
          />
          <DetailField label="ARTWORK URI" value={module.artworkUri} />
          <DetailField label="ARTWORK DIGEST" value={module.artworkDigest} />
          <DetailField label="SCHEMA" value={`CABALMESH V${module.schemaVersion}`} />
        </div>
      ) : null}
    </ModalDialog>
  );
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
