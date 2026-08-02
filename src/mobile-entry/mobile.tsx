/**
 * Mobile entry point.
 *
 * Separate from the desktop entry, and deliberately importing nothing from the
 * frozen tree: Tailwind and framer-motion live there, and both are hostile to
 * the design system. Tailwind compiles to the raw px and hex values the
 * adherence lint bans, and the brand forbids spring physics outright.
 *
 * Screens land here from ticket 29 onward.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import "../ds/index";

/**
 * Proves the vendored design system renders: real faces, real tokens, real
 * components. Replaced by the shell in ticket 28 and the screens from 29.
 */
function Placeholder() {
  return (
    <main
      style={{
        padding: "var(--space-8)",
        paddingTop: "calc(var(--safe-top) + var(--space-8))",
        background: "var(--surface-page)",
        color: "var(--text-secondary)",
        minHeight: "100vh",
      }}
    >
      <h1
        style={{
          fontFamily: "var(--type-wordmark-family)",
          letterSpacing: "var(--type-wordmark-tracking)",
          fontSize: "var(--text-lg)",
          color: "var(--text-primary)",
        }}
      >
        CABAL MESH
      </h1>
      <p style={{ marginTop: "var(--space-6)", fontSize: "var(--text-base)" }}>
        Design system vendored. Shell lands in ticket 28.
      </p>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Placeholder />
  </React.StrictMode>,
);
