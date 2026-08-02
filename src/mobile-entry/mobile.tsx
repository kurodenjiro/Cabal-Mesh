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

function Placeholder() {
  return (
    <main>
      <h1>CABAL MESH</h1>
      <p>Mobile shell. Screens land from ticket 29.</p>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Placeholder />
  </React.StrictMode>,
);
