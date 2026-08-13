/**
 * Mobile entry point.
 *
 * Imports nothing from the frozen desktop tree: Tailwind and framer-motion live
 * there, and both are hostile to the design system — Tailwind compiles to the
 * raw px and hex values the adherence lint bans, and the brand forbids spring
 * physics outright.
 */
import React, { useCallback, useState } from "react";
import ReactDOM from "react-dom/client";
import "../ds/index";
import { AppShell } from "../shell/AppShell";
import { Splash } from "../screens/Splash";
import { Unlock } from "../screens/Unlock";
import { Home } from "../screens/Home";
import { Market } from "../screens/Market";
import { Intents } from "../screens/Intents";
import { New } from "../screens/New";
import { Detail } from "../screens/Detail";
import { Settled } from "../screens/Settled";
import { Vault } from "../screens/Vault";
import { Profile } from "../screens/Profile";
import type { Screen } from "../shell/screen";

function App() {
  // Starts at splash: the app has no session until the user asks for one.
  // The mesh itself is already joined by the time this renders — bootstrap
  // does that at process startup, not on a user action — so there is nothing
  // to wait for between splash and home.
  const [screen, setScreen] = useState<Screen>({ name: "splash" });
  // The vault gate. Every screen past this point reads keys, so none of them
  // can render until one has been supplied — a locked vault is not a state the
  // rest of the app has a sensible rendering for.
  const [unlocked, setUnlocked] = useState(false);
  const onUnlocked = useCallback(() => setUnlocked(true), []);

  if (screen.name === "splash") {
    return <Splash onEnter={() => setScreen({ name: "home" })} />;
  }

  if (!unlocked) {
    return <Unlock onUnlocked={onUnlocked} />;
  }

  return (
    <AppShell screen={screen} onNavigate={setScreen}>
      {screen.name === "home" ? (
        <Home />
      ) : screen.name === "market" ? (
        <Market />
      ) : screen.name === "intents" ? (
        <Intents
          tab={screen.tab}
          onTabChange={(tab) => setScreen({ name: "intents", tab })}
          onCompose={() => setScreen({ name: "new" })}
        />
      ) : screen.name === "new" ? (
        <New onBroadcast={(id) => setScreen({ name: "detail", id })} />
      ) : screen.name === "detail" ? (
        // Settling navigates on, but only forward: the detail screen reports
        // that it settled and this decides, so a stale render cannot bounce a
        // user off a screen they navigated back to.
        <Detail id={screen.id} onSettled={(id) => setScreen({ name: "settled", id })} />
      ) : screen.name === "settled" ? (
        <Settled id={screen.id} onDone={() => setScreen({ name: "intents", tab: "HISTORY" })} />
      ) : screen.name === "vault" ? (
        <Vault tab={screen.tab} onTabChange={(tab) => setScreen({ name: "vault", tab })} />
      ) : screen.name === "profile" ? (
        <Profile onLeave={() => setScreen({ name: "splash" })} />
      ) : null}
    </AppShell>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
