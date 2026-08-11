import React, { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Input, Panel } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import { errorCopy } from "../state/errorCopy";
import type { IntentId } from "../shell/screen";
import type {
  AssetOption,
  FormOptions,
  IntentComposition,
  IntentCompositionStatus,
  IntentFields,
  IntentPreview,
} from "../types/bindings";

/**
 * Composing an intent.
 *
 * **Every option comes from Rust.** Actions, assets and their precision, the
 * conditions, the execution modes *and their descriptions*, the privacy levels.
 * Hardcoding any of them here would let a mode and its description drift apart,
 * and would put the decimal precision of an asset in two places — one of which
 * would eventually be wrong about money.
 *
 * **The review rows are computed server-side.** This screen never assembles the
 * summary it shows. It sends the fields, Rust parses them into a domain draft
 * and renders the rows *from that draft*, and broadcasting re-parses the same
 * fields through the same function. So the dialog cannot describe one intent
 * while a different one goes out — which a locally-built summary could, and
 * eventually would.
 *
 * **The closing line is Rust's too**, because it depends on whether this will
 * actually broadcast. See ticket 04: the prototype's single string claimed
 * offline intents settle on-chain, and they do not.
 *
 * **Conversation text never becomes an execution command.** `propose_intent`
 * has no app-state argument and returns only typed candidate fields. This
 * screen enables review only for a candidate that Rust marked validated, and
 * confirmation still goes through the existing preview/broadcast boundary.
 * The manual fields remain an explicit fallback if local inference cannot run.
 */
export function New({ onBroadcast }: { onBroadcast: (id: IntentId) => void }) {
  const [intentText, setIntentText] = useState("");
  const [submittedText, setSubmittedText] = useState<string | null>(null);
  const [composition, setComposition] = useState<IntentComposition | null>(null);
  const [inferenceBusy, setInferenceBusy] = useState(false);
  const [manualMode, setManualMode] = useState(false);
  const fieldsClaimed = useRef(false);

  const [options, setOptions] = useState<FormOptions | null>(null);
  const [action, setAction] = useState("BUY");
  const [asset, setAsset] = useState("AVAX");
  const [condition, setCondition] = useState("Price under");
  const [price, setPrice] = useState("");
  const [amount, setAmount] = useState("");
  const [mode, setMode] = useState("SHARK MODE");
  const [privacy, setPrivacy] = useState("HIGH");

  const [preview, setPreview] = useState<IntentPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    invoke<FormOptions>("intent_form_options")
      .then((next) => {
        if (cancelled) return;
        setOptions(next);
        // Defaults come from the options themselves rather than from the
        // literals above, so a change in Rust cannot leave this screen holding
        // a value the backend will reject.
        if (!fieldsClaimed.current) {
          if (next.actions[0]) setAction(next.actions[0]);
          if (next.assets[0]) setAsset(next.assets[0].name);
          if (next.conditions[0]) setCondition(next.conditions[0]);
          if (next.modes[0]) setMode(next.modes[0].label);
          if (next.privacyLevels[0]) setPrivacy(next.privacyLevels[0]);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const selectedAsset: AssetOption | undefined = options?.assets.find((a) => a.name === asset);
  const selectedMode = options?.modes.find((m) => m.label === mode);
  const needsPrice = !condition.toLowerCase().startsWith("any");

  // One object, matching Rust's `IntentFields`. Preview and broadcast send
  // the identical value, which is what makes "the dialog describes what goes
  // out" a property rather than a promise.
  const fields: IntentFields = { action, asset, condition, price, amount, mode, privacy };

  const applyFields = (next: IntentFields) => {
    fieldsClaimed.current = true;
    setAction(next.action);
    setAsset(next.asset);
    setCondition(next.condition);
    setPrice(next.price);
    setAmount(next.amount);
    setMode(next.mode);
    setPrivacy(next.privacy);
  };

  const propose = async () => {
    const text = intentText.trim();
    if (!text || inferenceBusy) return;

    setInferenceBusy(true);
    setError(null);
    setPreview(null);
    setComposition(null);
    setSubmittedText(text);
    try {
      const next = await invoke<IntentComposition>("propose_intent", { text });
      setComposition(next);
      if (next.fields) applyFields(next.fields);
      if (next.status === "validated") setManualMode(false);
    } catch (failure) {
      // Transport failures keep the phrase editable and leave no reviewable
      // draft behind. The command itself has no ledger or broadcast access.
      setError(errorCopy(failure));
    } finally {
      setInferenceBusy(false);
    }
  };

  const canReview = manualMode || composition?.status === "validated";

  const review = async () => {
    if (!canReview) return;
    setError(null);
    try {
      setPreview(await invoke<IntentPreview>("preview_intent", { fields }));
    } catch (failure) {
      setError(errorCopy(failure));
    }
  };

  const broadcast = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const id = await invoke<string>("broadcast_intent", { fields });
      setPreview(null);
      onBroadcast(id);
    } catch (failure) {
      setPreview(null);
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <Panel label="DESCRIBE INTENT">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
          {submittedText ? (
            <div
              style={{
                alignSelf: "flex-end",
                maxWidth: "86%",
                padding: "var(--space-4) var(--space-5)",
                border: "var(--border-width) solid var(--border-default)",
                background: "var(--surface-raised)",
                color: "var(--text-primary)",
                overflowWrap: "anywhere",
              }}
            >
              {submittedText}
            </div>
          ) : null}
          <label
            htmlFor="intent-conversation"
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: "var(--text-muted)",
            }}
          >
            SAY WHAT YOU WANT TO DO
          </label>
          <Input
            id="intent-conversation"
            multiline
            rows={3}
            value={intentText}
            maxLength={512}
            autoComplete="off"
            spellCheck="false"
            placeholder="buy 10 avax under 95, shark mode, privacy high"
            onChange={(event) => setIntentText((event.target as HTMLTextAreaElement).value)}
          />
          <Button
            tone="signal"
            size="md"
            block
            className="cm-touch"
            disabled={inferenceBusy || intentText.trim().length === 0}
            onClick={propose}
          >
            {inferenceBusy ? "PARSING LOCALLY" : "PARSE LOCALLY"}
          </Button>
          <span style={{ fontSize: "var(--text-2xs)", color: "var(--text-muted)" }}>
            TEXT STAYS ON THIS DEVICE. NOTHING IS SENT UNTIL YOU REVIEW AND CONFIRM.
          </span>
        </div>
      </Panel>

      {composition ? (
        <Panel label="◇ PARSED · LOCAL MODEL">
          <div
            aria-live="polite"
            style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}
          >
            {composition.chips.length > 0 ? (
              <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-3)" }}>
                {composition.chips.map((chip) => {
                  const label = fieldLabel(chip.field);
                  const missing = chip.value === null;
                  return (
                    <span
                      key={chip.field}
                      style={{
                        padding: "var(--space-3) var(--space-4)",
                        border: `var(--border-width) solid ${
                          missing ? "var(--accent-blood-red)" : "var(--border-default)"
                        }`,
                        color: missing ? "var(--accent-blood-red)" : "var(--text-primary)",
                        fontFamily: "var(--type-data-family)",
                        fontSize: "var(--text-xs)",
                        overflowWrap: "anywhere",
                      }}
                    >
                      <span style={{ color: "var(--text-muted)" }}>{label} · </span>
                      {chip.value ?? "MISSING"}
                    </span>
                  );
                })}
              </div>
            ) : null}
            <p style={{ margin: 0, color: "var(--text-secondary)", fontSize: "var(--text-base)" }}>
              {compositionMessage(composition.status, composition.missing.map(fieldLabel))}
            </p>
            <span style={{ color: "var(--text-muted)", fontSize: "var(--text-2xs)" }}>
              MODEL {composition.modelVersion}
            </span>
          </div>
        </Panel>
      ) : null}

      <Button
        tone="ghost"
        size="sm"
        block
        className="cm-touch"
        onClick={() => setManualMode((shown) => !shown)}
      >
        {manualMode ? "HIDE MANUAL FIELDS" : "USE MANUAL FIELDS"}
      </Button>

      {manualMode ? (
        <>
      <Panel label="I WANT TO">
        <div style={{ padding: "var(--space-6)" }}>
          <Segmented name="Action" options={options?.actions ?? []} value={action} onChange={setAction} />
        </div>
      </Panel>

      <Panel label="ASSET">
        <div style={{ padding: "var(--space-6)" }}>
          <Segmented
            name="Asset"
            options={(options?.assets ?? []).map((a) => a.name)}
            value={asset}
            onChange={setAsset}
          />
        </div>
      </Panel>

      <Panel label="CONDITION">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          <Segmented
            name="Condition"
            options={options?.conditions ?? []}
            value={condition}
            onChange={setCondition}
          />
          {needsPrice ? (
            <DecimalField
              id="intent-price"
              label="PRICE (USD)"
              value={price}
              onChange={setPrice}
              prefix="$"
              placeholder="95.00"
            />
          ) : null}
        </div>
      </Panel>

      <Panel label="AMOUNT">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
          <DecimalField
            id="intent-amount"
            label={`AMOUNT (${asset})`}
            value={amount}
            onChange={setAmount}
            placeholder="0.0"
            suffix={
              selectedAsset?.available ? (
                <button
                  type="button"
                  className="cm-touch"
                  onClick={() => setAmount(selectedAsset.available ?? "")}
                  style={{
                    background: "none",
                    border: "none",
                    padding: 0,
                    cursor: "pointer",
                    font: "inherit",
                    color: "var(--text-primary)",
                    letterSpacing: "var(--tracking-widest)",
                  }}
                >
                  MAX
                </button>
              ) : undefined
            }
          />
          {/* Absent, not zero. An asset this wallet has never held has no
              known balance, and "0" would claim it holds none. */}
          <span style={{ fontSize: "var(--text-2xs)", letterSpacing: "var(--tracking-widest)", color: "var(--text-muted)" }}>
            {selectedAsset?.available ? `AVAILABLE ${selectedAsset.available} ${asset}` : "BALANCE NOT KNOWN"}
          </span>
        </div>
      </Panel>

      <Panel label="MODE">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
          <Segmented
            name="Execution mode"
            options={(options?.modes ?? []).map((m) => m.label)}
            value={mode}
            onChange={setMode}
          />
          {/* The description travels with the mode from Rust, so the two
              cannot describe different strategies. */}
          <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>
            {selectedMode?.description ?? ""}
          </span>
        </div>
      </Panel>

      <Panel label="PRIVACY">
        <div style={{ padding: "var(--space-6)" }}>
          <Segmented
            name="Privacy level"
            options={options?.privacyLevels ?? []}
            value={privacy}
            onChange={setPrivacy}
          />
        </div>
      </Panel>
        </>
      ) : null}

      {error ? (
        <p role="alert" style={{ fontSize: "var(--text-base)", color: "var(--accent-blood-red)" }}>
          {error}
        </p>
      ) : null}

      <Button tone="primary" size="lg" block className="cm-touch" disabled={!canReview} onClick={review}>
        REVIEW INTENT
      </Button>

      <ModalDialog
        open={preview !== null}
        title="CONFIRM INTENT"
        onClose={() => setPreview(null)}
        footer={
          <>
            <Button tone="ghost" size="md" className="cm-touch" onClick={() => setPreview(null)}>
              CANCEL
            </Button>
            <Button tone="primary" size="md" className="cm-touch" disabled={busy} onClick={broadcast}>
              {/* The verb matches the prose above it, and both come from the
                  same answer about whether the mesh is reachable. */}
              {preview?.willBroadcast ? "BROADCAST" : "QUEUE LOCALLY"}
            </Button>
          </>
        }
      >
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          {(preview?.rows ?? []).map((row) => (
            <div key={row.key} style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-5)" }}>
              <span
                style={{
                  fontFamily: "var(--type-label-family)",
                  fontSize: "var(--text-2xs)",
                  letterSpacing: "var(--tracking-widest)",
                  color: "var(--text-muted)",
                }}
              >
                {row.key}
              </span>
              <span style={{ fontFamily: "var(--type-data-family)", color: "var(--text-primary)" }}>{row.value}</span>
            </div>
          ))}
          <p style={{ fontSize: "var(--text-base)", color: "var(--text-secondary)", margin: 0 }}>
            {preview?.confirm}
          </p>
        </div>
      </ModalDialog>
    </div>
  );
}

function fieldLabel(field: IntentComposition["missing"][number]): string {
  return field.replace(/_/g, " ").toUpperCase();
}

function compositionMessage(status: IntentCompositionStatus, missing: string[]): string {
  switch (status) {
    case "validated":
      return "All six fields passed Rust domain validation. Review before anything leaves this device.";
    case "needs_clarification":
      return missing.length > 0
        ? `Clarify: ${missing.join(", ")}. No financial value was invented.`
        : "Clarify the phrase. No financial value was invented.";
    case "unavailable":
      return "The local model is unavailable. Your text remains editable; use manual fields or retry. Nothing was sent.";
    case "timed_out":
      return "The local model timed out. Your text remains editable; use manual fields or retry. Nothing was sent.";
    case "malformed_output":
      return "Rust refused the model output. Revise the phrase or use manual fields. Nothing was sent.";
    case "refused":
      return "The text was refused at the private inference boundary. Revise it or use manual fields. Nothing was sent.";
  }
}

/**
 * The board's segmented control, wrapping rather than overflowing.
 *
 * `flexWrap` is the whole point: five uppercase labels in 390px at 200% font
 * scale is far past the available width, and a row that scrolls horizontally
 * hides options with no affordance saying so. Wrapping keeps every option
 * reachable at every scale.
 */
function Segmented({
  name,
  options,
  value,
  onChange,
}: {
  name: string;
  options: string[];
  value: string;
  onChange: (next: string) => void;
}) {
  return (
    // radiogroup, not tablist: these choose a value, they do not switch views,
    // and the distinction is what a screen reader announces.
    <div role="radiogroup" aria-label={name} style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-3)" }}>
      {options.map((option) => {
        const selected = option === value;
        return (
          <button
            key={option}
            type="button"
            role="radio"
            aria-checked={selected}
            className="cm-touch"
            onClick={() => onChange(option)}
            style={{
              flex: "1 1 auto",
              padding: "var(--space-4) var(--space-5)",
              background: selected ? "var(--surface-raised)" : "none",
              border: selected
                ? "var(--border-width) solid var(--border-loud)"
                : "var(--border-width) solid var(--border-hairline)",
              color: selected ? "var(--text-primary)" : "var(--text-muted)",
              cursor: "pointer",
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              textTransform: "uppercase",
            }}
          >
            {option}
          </button>
        );
      })}
    </div>
  );
}

/**
 * A numeric field that behaves on a phone.
 *
 * `inputMode="decimal"` rather than `type="number"`: number inputs bring up a
 * keypad with no decimal separator on some Android keyboards, and silently
 * discard non-numeric input rather than letting Rust reject it with a reason.
 *
 * On focus the field scrolls itself into view. The keyboard opens over the
 * bottom half of the screen without resizing the layout viewport, so a field in
 * that half is simply hidden behind it with nothing to indicate why.
 */
function DecimalField({
  id,
  label,
  value,
  onChange,
  prefix,
  suffix,
  placeholder,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (next: string) => void;
  prefix?: string;
  suffix?: React.ReactNode;
  placeholder?: string;
}) {
  const wrapper = useRef<HTMLDivElement | null>(null);

  return (
    <div ref={wrapper} style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
      <label
        htmlFor={id}
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-muted)",
        }}
      >
        {label}
      </label>
      <Input
        id={id}
        value={value}
        inputMode="decimal"
        // A pattern rather than a filter: rejecting keystrokes here would mean
        // reimplementing the parser Rust already owns, and disagreeing with it.
        pattern="[0-9]*[.,]?[0-9]*"
        autoComplete="off"
        placeholder={placeholder}
        prefix={prefix}
        suffix={suffix}
        onChange={(event) => onChange((event.target as HTMLInputElement).value)}
        onFocus={() => {
          // Deferred past the keyboard animation; scrolling while it is still
          // opening lands on the wrong position.
          window.setTimeout(() => {
            wrapper.current?.scrollIntoView({ block: "center", behavior: "smooth" });
          }, 300);
        }}
      />
    </div>
  );
}
