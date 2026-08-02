/**
 * Mobile entry point.
 *
 * Imports nothing from the frozen desktop tree: Tailwind and framer-motion live
 * there, and both are hostile to the design system — Tailwind compiles to the
 * raw px and hex values the adherence lint bans, and the brand forbids spring
 * physics outright.
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import "../ds/index";
import { AppShell } from "../shell/AppShell";
import type { Screen } from "../shell/screen";

/**
 * Per-screen bodies land from ticket 29. Until then each renders its own name,
 * which is enough to verify navigation, safe areas, tab semantics and the type
 * scale on a device.
 */
function Placeholder({ screen }: { screen: Screen }) {
  return (
    <section
      style={{
        padding: "var(--space-8)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-5)",
      }}
    >
      <p
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-muted)",
          textTransform: "uppercase",
        }}
      >
        {screen.name}
      </p>
      <p style={{ fontSize: "var(--text-base)" }}>
        Shell in place. This screen lands in ticket 29.
      </p>
    </section>
  );
}

function App() {
  const [screen, setScreen] = useState<Screen>({ name: "home" });

  return (
    <AppShell screen={screen} onNavigate={setScreen}>
      <Placeholder screen={screen} />
    </AppShell>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
