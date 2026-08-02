import { useRef, useState } from "react";
import { Terminal } from "../ds";
import { useLogStream } from "../state/useLogStream";
import type { LogLine } from "../types/bindings";

/** Retained lines. Four are visible; the rest scroll. */
const RING_CAPACITY = 200;

/**
 * The handshake, streamed from Rust.
 *
 * The prototype fakes this with a `setInterval` over a canned array. Here it is
 * a real `Channel`: the lines arrive as the mesh actually joins, and the stream
 * is torn down when the screen unmounts.
 *
 * Leaving this screen cancels *delivery*, never the join — the mesh stays up.
 */
export function Connecting({ onJoined }: { onJoined: () => void }) {
  const [lines, setLines] = useState<LogLine[]>([]);
  const joined = useRef(false);

  useLogStream("enter_mesh", {}, (line) => {
    setLines((previous) => {
      // Bounded: a live feed into an unbounded array is a slow OOM on a 2 GB
      // device.
      const next = [...previous, line];
      return next.length > RING_CAPACITY ? next.slice(-RING_CAPACITY) : next;
    });

    // The final line is the join. Guarded so a duplicate cannot navigate twice.
    if (line.tone === "ok" && !joined.current) {
      joined.current = true;
      window.setTimeout(onJoined, 600);
    }
  });

  const progress = Math.min(100, Math.round((lines.length / 5) * 100));

  return (
    <section
      style={{
        minHeight: "100dvh",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-7)",
        padding: "var(--space-9) var(--space-8)",
        paddingTop: "calc(var(--safe-top) + var(--space-9))",
        paddingBottom: "calc(var(--safe-bottom) + var(--space-9))",
      }}
    >
      <h1
        style={{
          margin: 0,
          fontFamily: "var(--type-heading-family)",
          fontSize: "var(--text-md)",
          letterSpacing: "var(--type-heading-tracking)",
          color: "var(--text-primary)",
        }}
      >
        CONNECTING TO MESH
      </h1>

      <p
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-muted)",
          textTransform: "uppercase",
        }}
      >
        No identity is attached.
      </p>

      {/* Stepped, not smooth: the brand's motion is mechanical, and a meter
          that eases would be off-brand even if it looked fine. */}
      <div
        role="progressbar"
        aria-valuenow={progress}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Handshake progress"
        style={{
          height: 4,
          background: "var(--surface-raised)",
          border: "var(--border-hairline-style)",
        }}
      >
        <div
          style={{
            width: `${progress}%`,
            height: "100%",
            background: "var(--ink-white)",
            transition: "width var(--dur-slow) var(--ease-step)",
          }}
        />
      </div>

      <div style={{ flex: 1, minHeight: 0 }}>
        <Terminal
          label="HANDSHAKE LOG"
          // Polite, never assertive: a log that interrupts every announcement
          // is worse than one nobody hears.
          role="log"
          aria-live="polite"
          lines={lines.map((line) => ({ text: line.text, tone: line.tone }))}
        />
      </div>
    </section>
  );
}
