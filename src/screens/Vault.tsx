import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IconButton, Panel } from "../ds";
import type { VaultTab } from "../shell/screen";
import type { VaultRow } from "../types/bindings";

const TABS: VaultTab[] = ["ASSETS", "IDENTITIES", "KEYS"];

const COMMAND: Record<VaultTab, string> = {
  ASSETS: "vault_assets",
  IDENTITIES: "vault_identities",
  KEYS: "vault_keys",
};

/**
 * Assets, identities and key metadata.
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
  const [revealed, setRevealed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    invoke<VaultRow[]>(COMMAND[tab])
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

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <div role="tablist" aria-label="Vault section" style={{ display: "flex", gap: "var(--space-7)" }}>
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

      {tab === "ASSETS" && (
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
      )}

      <Panel label={tab}>
        {rows === null ? null : rows.length === 0 ? (
          <Empty tab={tab} />
        ) : (
          rows.map((row) => <Row key={`${row.tag}-${row.name}`} row={row} />)
        )}
      </Panel>
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

function Empty({ tab }: { tab: VaultTab }) {
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
