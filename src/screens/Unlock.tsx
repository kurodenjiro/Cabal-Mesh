import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Panel } from "../ds";
import type { VaultStatusView, VaultUnlockView } from "../types/bindings";

/**
 * The passphrase that opens the vault, and the one that creates it.
 *
 * This screen exists because the vault key used to sit beside the vault as
 * plain hex, which meant anything running as this user held the wallet. The
 * key is now encrypted under something only the owner knows, and something
 * only the owner knows has to be asked for.
 *
 * **There is no reset.** The copy says so before the passphrase is chosen
 * rather than after it is forgotten, because afterwards is too late to be
 * useful — Argon2id over a random salt is not reversible by the people who
 * wrote it.
 */
export function Unlock({ onUnlocked }: { onUnlocked: () => void }) {
  const [status, setStatus] = useState<VaultStatusView | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [outcome, setOutcome] = useState<VaultUnlockView | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  // Bootstrap publishes its services a moment after the process starts, and
  // this screen can be reached before then. Retrying is the whole handling:
  // "not ready" is a stage, not a fault.
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    const poll = () => {
      invoke<VaultStatusView>("vault_status")
        .then((next) => {
          if (cancelled) return;
          setStatus(next);
          if (next.status === "unlocked") onUnlocked();
        })
        .catch(() => {
          if (!cancelled) timer = setTimeout(poll, 400);
        });
    };
    poll();

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [onUnlocked]);

  const creating = status?.status === "uninitialized";
  const mismatched = creating && confirmation.length > 0 && passphrase !== confirmation;
  const tooShort = creating && passphrase.length > 0 && passphrase.length < 12;
  const submittable =
    passphrase.length > 0 && !busy && (!creating || (passphrase === confirmation && !tooShort));

  const submit = useCallback(async () => {
    if (!submittable) return;
    setBusy(true);
    setFailed(false);
    setOutcome(null);
    try {
      const result = await invoke<VaultUnlockView>("unlock_vault", { passphrase });
      setOutcome(result);
      if (result.status === "unlocked") {
        // Dropped the instant it is no longer needed. Holding it would leave
        // the passphrase in a mounted component for the rest of the session.
        setPassphrase("");
        setConfirmation("");
        onUnlocked();
      }
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }, [onUnlocked, passphrase, submittable]);

  return (
    <div
      style={{
        minHeight: "100dvh",
        display: "flex",
        flexDirection: "column",
        justifyContent: "center",
        gap: "var(--space-6)",
        padding: "var(--space-6)",
        background: "var(--surface-base)",
      }}
    >
      <Panel label={creating ? "CHOOSE A PASSPHRASE" : "UNLOCK THE VAULT"}>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
          style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}
        >
          <p style={{ margin: 0, fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>
            {status === null
              ? "Reading the vault."
              : creating
                ? "Your keys are encrypted with this passphrase. It is never stored and cannot be reset — if you forget it, everything in this wallet is gone unless you exported the key first."
                : "Your keys are encrypted with this passphrase. Nothing on this device can read them without it."}
          </p>

          <label style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
            <span style={labelStyle}>PASSPHRASE</span>
            <input
              type="password"
              autoComplete={creating ? "new-password" : "current-password"}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              // eslint-disable-next-line jsx-a11y/no-autofocus -- the only
              // control on the only screen the app can show right now.
              autoFocus
              value={passphrase}
              disabled={busy || status === null}
              aria-invalid={tooShort}
              onChange={(event) => setPassphrase(event.currentTarget.value)}
              style={inputStyle}
            />
            {creating ? (
              <span style={{ fontSize: "var(--text-xs)", color: tooShort ? "var(--accent-blood-red)" : "var(--text-muted)" }}>
                At least 12 characters. Length beats punctuation — a short passphrase with symbols in it is still short.
              </span>
            ) : null}
          </label>

          {creating ? (
            <label style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
              <span style={labelStyle}>REPEAT IT</span>
              <input
                type="password"
                autoComplete="new-password"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                value={confirmation}
                disabled={busy}
                aria-invalid={mismatched}
                onChange={(event) => setConfirmation(event.currentTarget.value)}
                style={{
                  ...inputStyle,
                  borderColor: mismatched ? "var(--accent-blood-red)" : undefined,
                }}
              />
              {mismatched ? (
                <span style={{ fontSize: "var(--text-xs)", color: "var(--accent-blood-red)" }}>
                  These do not match. A typo saved now is a wallet lost later.
                </span>
              ) : null}
            </label>
          ) : null}

          {failed ? <Notice title="COULD NOT REACH THE VAULT" body="Nothing was changed. Try again." alert /> : null}

          {outcome?.status === "wrong_secret" ? (
            <Notice
              title="THAT IS NOT THE PASSPHRASE"
              body="Nothing was changed and nothing was destroyed. Check your capitals and your keyboard language."
              alert
            />
          ) : outcome?.status === "rate_limited" ? (
            <Notice
              title="TOO MANY ATTEMPTS"
              body={`Wait ${formatDelay(Number(outcome.retryInSeconds))} before trying again. This delay is a speed bump on this device, not protection for the file itself.`}
              alert
            />
          ) : outcome?.status === "device_binding_unavailable" ? (
            <Notice
              title="THIS KEY BELONGS TO ANOTHER DEVICE"
              body="Part of it was kept in the key store of the device that made it, so copying the files was never going to be enough. Your passphrase may well be correct. Open it on that device, or restore this wallet from an exported key."
              alert
            />
          ) : outcome?.status === "unusable" ? (
            <Notice
              title="THE STORED KEY CANNOT BE READ"
              body="Retyping will not help. The key file is damaged, or was written by a newer build. Restore this wallet from an exported key on a fresh install."
              alert
            />
          ) : null}

          <Button type="submit" tone="primary" size="lg" className="cm-touch" disabled={!submittable}>
            {busy ? "DERIVING KEY" : creating ? "CREATE THE VAULT" : "UNLOCK"}
          </Button>

          {creating ? (
            <p style={{ margin: 0, fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>
              Write it down somewhere physical. There is no recovery question, no reset link, and nobody to ask.
            </p>
          ) : null}
        </form>
      </Panel>
    </div>
  );
}

const labelStyle = {
  fontFamily: "var(--type-label-family)",
  fontSize: "var(--text-2xs)",
  letterSpacing: "var(--tracking-widest)",
  color: "var(--text-secondary)",
} as const;

const inputStyle = {
  minHeight: "var(--control-min-height)",
  border: "var(--border-hairline-style)",
  background: "var(--surface-sunken)",
  color: "var(--text-primary)",
  fontFamily: "var(--type-data-family)",
  fontSize: "var(--text-base)",
  padding: "var(--space-3) var(--space-4)",
} as const;

function formatDelay(seconds: number): string {
  if (seconds < 60) return `${Math.max(1, Math.ceil(seconds))} seconds`;
  return `${Math.ceil(seconds / 60)} minutes`;
}

function Notice({ title, body, alert = false }: { title: string; body: string; alert?: boolean }) {
  return (
    <div role={alert ? "alert" : undefined} style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
      <span style={{ ...labelStyle, color: alert ? "var(--accent-blood-red)" : "var(--text-primary)" }}>{title}</span>
      <span style={{ fontSize: "var(--text-sm)", color: "var(--text-muted)" }}>{body}</span>
    </div>
  );
}
