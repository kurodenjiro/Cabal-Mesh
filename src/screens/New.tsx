import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Input, Panel } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import { errorCopy } from "../state/errorCopy";
import type { IntentId } from "../shell/screen";
import type {
  AssetOption,
  FormOptions,
  IntentAffordability,
  IntentComposition,
  IntentCompositionStatus,
  IntentFields,
  IntentFieldView,
  IntentPreview,
} from "../types/bindings";

const EDITABLE_FIELDS: IntentFieldView[] = ["action", "asset", "condition", "amount", "mode", "privacy"];

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
 * screen enables review only after every candidate field is supplied, and
 * confirmation still goes through the existing preview/broadcast boundary.
 * The exact fields Rust previewed are frozen while the dialog is open, so a
 * later render cannot substitute live form state at confirmation time. The
 * manual fields remain an explicit fallback if local inference cannot run.
 */
export function New({ onBroadcast }: { onBroadcast: (id: IntentId) => void }) {
  const [intentText, setIntentText] = useState("");
  const [submittedText, setSubmittedText] = useState<string | null>(null);
  const [composition, setComposition] = useState<IntentComposition | null>(null);
  const [userEditedComposition, setUserEditedComposition] = useState(false);
  const [editingField, setEditingField] = useState<IntentFieldView | null>(null);
  const [editDraft, setEditDraft] = useState<IntentFields | null>(null);
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
  const [reviewedFields, setReviewedFields] = useState<IntentFields | null>(null);
  const [reviewBusy, setReviewBusy] = useState(false);
  const reviewSequence = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const closePreview = useCallback(() => {
    setPreview(null);
    setReviewedFields(null);
  }, []);
  const closeEditor = useCallback(() => {
    setEditingField(null);
    setEditDraft(null);
  }, []);

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
  const liveAffordability = useIntentAffordability(
    asset,
    amount,
    manualMode || composition?.fields != null,
  );
  const editAffordability = useIntentAffordability(
    editDraft?.asset ?? "",
    editDraft?.amount ?? "",
    editingField === "amount",
  );

  // Any field change invalidates an in-flight or open preview. A slow preview
  // response must never reopen a dialog for fields the user has since changed.
  useEffect(() => {
    reviewSequence.current += 1;
    setReviewBusy(false);
    closePreview();
  }, [action, amount, asset, closePreview, condition, mode, price, privacy]);

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
    reviewSequence.current += 1;
    setError(null);
    closePreview();
    closeEditor();
    setComposition(null);
    setUserEditedComposition(false);
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

  const hasEditableFields = manualMode || composition?.fields != null;
  const missing = hasEditableFields ? missingIntentFields(fields) : (composition?.missing ?? []);
  const canReview = hasEditableFields && missing.length === 0 && !inferenceBusy && !reviewBusy;
  const conversationChips = composition?.fields
    ? EDITABLE_FIELDS.map((field) => ({ field, value: chipValue(field, fields) }))
    : (composition?.chips ?? []);
  const compositionCopy = composition
    ? compositionMessage(
        composition.status,
        missing.map(fieldLabel),
        composition.fields != null,
        userEditedComposition,
      )
    : "";
  const liveAvailable = liveAffordability.feedback
    ? liveAffordability.feedback.available
    : (selectedAsset?.available ?? null);
  const editSelectedAsset = options?.assets.find((candidate) => candidate.name === editDraft?.asset);
  const editAvailable = editAffordability.feedback
    ? editAffordability.feedback.available
    : (editSelectedAsset?.available ?? null);

  const openEditor = (field: IntentFieldView) => {
    reviewSequence.current += 1;
    setReviewBusy(false);
    setError(null);
    closePreview();
    setEditDraft({ ...fields });
    setEditingField(field);
  };

  const saveEditor = () => {
    if (!editDraft) return;
    applyFields(editDraft);
    setUserEditedComposition(true);
    closeEditor();
  };

  const review = async () => {
    if (!canReview) return;
    setError(null);
    const candidate = { ...fields };
    const sequence = ++reviewSequence.current;
    setReviewBusy(true);
    try {
      const nextPreview = await invoke<IntentPreview>("preview_intent", { fields: candidate });
      if (sequence !== reviewSequence.current) return;
      setReviewedFields(candidate);
      setPreview(nextPreview);
    } catch (failure) {
      if (sequence !== reviewSequence.current) return;
      setReviewedFields(null);
      setError(errorCopy(failure));
    } finally {
      if (sequence === reviewSequence.current) setReviewBusy(false);
    }
  };

  const broadcast = async () => {
    if (busy || !reviewedFields) return;
    setBusy(true);
    try {
      const id = await invoke<string>("broadcast_intent", { fields: reviewedFields });
      closePreview();
      onBroadcast(id);
    } catch (failure) {
      closePreview();
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
            style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}
          >
            {conversationChips.length > 0 ? (
              <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-3)" }}>
                {conversationChips.map((chip) => {
                  const label = fieldLabel(chip.field);
                  const missing = chip.value === null;
                  return (
                    <button
                      key={chip.field}
                      type="button"
                      className="cm-touch"
                      disabled={composition.fields === null || options === null}
                      aria-expanded={editingField === chip.field}
                      aria-haspopup="dialog"
                      aria-label={`Edit ${label}: ${chip.value ?? "missing"}`}
                      onClick={() => openEditor(chip.field)}
                      style={{
                        padding: "var(--space-3) var(--space-4)",
                        border: `var(--border-width) solid ${
                          missing ? "var(--accent-blood-red)" : "var(--border-default)"
                        }`,
                        background: "none",
                        color: missing ? "var(--accent-blood-red)" : "var(--text-primary)",
                        cursor: composition.fields === null || options === null ? "not-allowed" : "pointer",
                        fontFamily: "var(--type-data-family)",
                        fontSize: "var(--text-xs)",
                        overflowWrap: "anywhere",
                        textAlign: "left",
                      }}
                    >
                      <span style={{ color: "var(--text-muted)" }}>{label} · </span>
                      {chip.value ?? "MISSING"}
                    </button>
                  );
                })}
              </div>
            ) : null}
            {composition.fields ? (
              <span style={{ color: "var(--text-muted)", fontSize: "var(--text-2xs)" }}>
                TAP A CHIP TO CHANGE IT. CHANGES STAY LOCAL UNTIL REVIEW.
              </span>
            ) : null}
            <p
              aria-live="polite"
              style={{ margin: 0, color: "var(--text-secondary)", fontSize: "var(--text-base)" }}
            >
              {compositionCopy}
            </p>
            <span style={{ color: "var(--text-muted)", fontSize: "var(--text-2xs)" }}>
              MODEL {composition.modelVersion}
            </span>
          </div>
        </Panel>
      ) : null}

      {composition?.fields ? (
        <BalanceNotice
          asset={asset}
          available={liveAvailable}
          feedback={liveAffordability.feedback}
          checking={liveAffordability.checking}
        />
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
            describedBy="intent-amount-balance"
            suffix={
              liveAvailable !== null ? (
                <button
                  type="button"
                  className="cm-touch"
                  onClick={() => setAmount(liveAvailable)}
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
          <span
            id="intent-amount-balance"
            role={liveAffordability.feedback?.status === "shortfall" ? "alert" : "status"}
            aria-live="polite"
            style={{
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color:
                liveAffordability.feedback?.status === "shortfall"
                  ? "var(--accent-blood-red)"
                  : "var(--text-muted)",
            }}
          >
            {balanceMessage(asset, liveAvailable, liveAffordability.feedback, liveAffordability.checking)}
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
        {reviewBusy ? "VALIDATING INTENT" : "REVIEW INTENT"}
      </Button>

      <ModalDialog
        open={editingField !== null && editDraft !== null}
        title={`EDIT ${editingField ? fieldLabel(editingField) : "FIELD"}`}
        onClose={closeEditor}
        footer={
          <>
            <Button tone="ghost" size="md" className="cm-touch" onClick={closeEditor}>
              CANCEL
            </Button>
            <Button tone="primary" size="md" className="cm-touch" onClick={saveEditor}>
              SET
            </Button>
          </>
        }
      >
        {editingField && editDraft ? (
          <IntentFieldEditor
            field={editingField}
            fields={editDraft}
            options={options}
            affordability={editAffordability.feedback}
            affordabilityChecking={editAffordability.checking}
            available={editAvailable}
            onChange={setEditDraft}
          />
        ) : null}
      </ModalDialog>

      <ModalDialog
        open={preview !== null}
        title="CONFIRM INTENT"
        onClose={closePreview}
        footer={
          <>
            <Button tone="ghost" size="md" className="cm-touch" onClick={closePreview}>
              CANCEL
            </Button>
            <Button
              tone="primary"
              size="md"
              className="cm-touch"
              disabled={busy || reviewedFields === null}
              onClick={broadcast}
            >
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

function IntentFieldEditor({
  field,
  fields,
  options,
  affordability,
  affordabilityChecking,
  available,
  onChange,
}: {
  field: IntentFieldView;
  fields: IntentFields;
  options: FormOptions | null;
  affordability: IntentAffordability | null;
  affordabilityChecking: boolean;
  available: string | null;
  onChange: (next: IntentFields) => void;
}) {
  const update = (patch: Partial<IntentFields>) => onChange({ ...fields, ...patch });
  const mode = options?.modes.find((candidate) => candidate.label === fields.mode);

  switch (field) {
    case "action":
      return (
        <Segmented
          name="Action"
          options={options?.actions ?? []}
          value={fields.action}
          onChange={(action) => update({ action })}
        />
      );
    case "asset":
      return (
        <Segmented
          name="Asset"
          options={(options?.assets ?? []).map((candidate) => candidate.name)}
          value={fields.asset}
          onChange={(asset) => update({ asset })}
        />
      );
    case "condition":
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          <Segmented
            name="Condition"
            options={options?.conditions ?? []}
            value={fields.condition}
            onChange={(condition) => update({ condition })}
          />
          {!fields.condition.toLowerCase().startsWith("any") ? (
            <DecimalField
              id="chip-intent-price"
              label="PRICE (USD)"
              value={fields.price}
              onChange={(price) => update({ price })}
              prefix="$"
              placeholder="95.00"
            />
          ) : null}
        </div>
      );
    case "amount":
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
          <DecimalField
            id="chip-intent-amount"
            label={`AMOUNT (${fields.asset || "SELECT ASSET"})`}
            value={fields.amount}
            onChange={(amount) => update({ amount })}
            describedBy="chip-intent-amount-balance"
            placeholder="0.0"
            suffix={
              available !== null ? (
                <button
                  type="button"
                  className="cm-touch"
                  onClick={() => update({ amount: available })}
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
          <span
            id="chip-intent-amount-balance"
            role={affordability?.status === "shortfall" ? "alert" : "status"}
            aria-live="polite"
            style={{
              color:
                affordability?.status === "shortfall"
                  ? "var(--accent-blood-red)"
                  : "var(--text-muted)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
            }}
          >
            {balanceMessage(fields.asset, available, affordability, affordabilityChecking)}
          </span>
        </div>
      );
    case "mode":
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
          <Segmented
            name="Execution mode"
            options={(options?.modes ?? []).map((candidate) => candidate.label)}
            value={fields.mode}
            onChange={(mode) => update({ mode })}
          />
          <span style={{ color: "var(--text-muted)", fontSize: "var(--text-base)" }}>
            {mode?.description ?? ""}
          </span>
        </div>
      );
    case "privacy":
      return (
        <Segmented
          name="Privacy level"
          options={options?.privacyLevels ?? []}
          value={fields.privacy}
          onChange={(privacy) => update({ privacy })}
        />
      );
  }
}

function BalanceNotice({
  asset,
  available,
  feedback,
  checking,
}: {
  asset: string;
  available: string | null;
  feedback: IntentAffordability | null;
  checking: boolean;
}) {
  const shortfall = feedback?.status === "shortfall";
  return (
    <Panel label={shortfall ? "⚠ AFFORDABILITY" : "BALANCE"}>
      <p
        role={shortfall ? "alert" : "status"}
        aria-live="polite"
        style={{
          margin: 0,
          padding: "var(--space-5) var(--space-6)",
          color: shortfall ? "var(--accent-blood-red)" : "var(--text-secondary)",
          fontFamily: "var(--type-data-family)",
          fontSize: "var(--text-sm)",
          overflowWrap: "anywhere",
        }}
      >
        {balanceMessage(asset, available, feedback, checking)}
      </p>
    </Panel>
  );
}

function balanceMessage(
  asset: string,
  available: string | null,
  feedback: IntentAffordability | null,
  checking: boolean,
): string {
  if (checking) return "CHECKING LATEST BALANCE…";
  if (feedback?.status === "shortfall") {
    return `SHORT BY ${feedback.shortfall ?? "?"} ${asset} · AVAILABLE ${feedback.available ?? "?"} ${asset}`;
  }
  if (feedback?.status === "invalid_amount") {
    return available === null
      ? "ENTER A VALID AMOUNT · BALANCE NOT KNOWN"
      : `ENTER A VALID AMOUNT · AVAILABLE ${available} ${asset}`;
  }
  if (feedback?.status === "affordable" && available !== null) {
    return `AVAILABLE ${available} ${asset} · AMOUNT COVERED`;
  }
  return available === null ? "BALANCE NOT KNOWN" : `AVAILABLE ${available} ${asset}`;
}

function useIntentAffordability(asset: string, amount: string, active: boolean) {
  const [feedback, setFeedback] = useState<IntentAffordability | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    if (!active || !asset) {
      setFeedback(null);
      setChecking(false);
      return;
    }

    let cancelled = false;
    setFeedback(null);
    setChecking(true);
    const timer = window.setTimeout(() => {
      invoke<IntentAffordability>("intent_affordability", { asset, amount })
        .then((next) => {
          if (!cancelled) setFeedback(next);
        })
        .catch(() => {
          if (!cancelled) setFeedback(null);
        })
        .finally(() => {
          if (!cancelled) setChecking(false);
        });
    }, 120);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [active, amount, asset]);

  return { feedback, checking };
}

function fieldLabel(field: IntentFieldView): string {
  return field.replace(/_/g, " ").toUpperCase();
}

function chipValue(field: IntentFieldView, fields: IntentFields): string | null {
  switch (field) {
    case "action":
      return fields.action || null;
    case "asset":
      return fields.asset || null;
    case "condition": {
      if (!fields.condition) return null;
      if (fields.condition.toLowerCase().startsWith("any")) return "ANY PRICE";
      if (!fields.price) return null;
      const direction = fields.condition.toLowerCase().includes("above") ? "ABOVE" : "UNDER";
      return `${direction} $${fields.price.replace(/^\$/, "")}`;
    }
    case "amount":
      return fields.amount ? `${fields.amount}${fields.asset ? ` ${fields.asset}` : ""}` : null;
    case "mode":
      return fields.mode || null;
    case "privacy":
      return fields.privacy || null;
  }
}

function missingIntentFields(fields: IntentFields): IntentFieldView[] {
  return EDITABLE_FIELDS.filter((field) => chipValue(field, fields) === null);
}

function compositionMessage(
  status: IntentCompositionStatus,
  missing: string[],
  hasFields: boolean,
  userEdited: boolean,
): string {
  if (hasFields && missing.length > 0) {
    return `Clarify: ${missing.join(", ")}. No financial value was invented.`;
  }
  if (hasFields && (userEdited || status === "needs_clarification")) {
    return "All six fields are supplied. Rust will validate this update before review.";
  }
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
      {options.map((option, index) => {
        const selected = option === value;
        return (
          <button
            key={option}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected || (!options.includes(value) && index === 0) ? 0 : -1}
            className="cm-touch"
            onClick={() => onChange(option)}
            onKeyDown={(event) => {
              if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) {
                return;
              }
              event.preventDefault();
              const nextIndex =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? options.length - 1
                    : (index + (event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1) + options.length) %
                      options.length;
              const next = options[nextIndex];
              if (!next) return;
              onChange(next);
              const buttons = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="radio"]');
              window.requestAnimationFrame(() => buttons?.[nextIndex]?.focus());
            }}
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
  describedBy,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (next: string) => void;
  prefix?: string;
  suffix?: React.ReactNode;
  placeholder?: string;
  describedBy?: string;
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
        aria-describedby={describedBy}
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
