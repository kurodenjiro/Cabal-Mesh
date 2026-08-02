import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

/**
 * Two builds, selected by mode.
 *
 * `--mode mobile` builds the design-system UI; the default builds the frozen
 * desktop RPG UI. They are separate because Tailwind and framer-motion are
 * actively hostile to the design system — Tailwind compiles to the raw px and
 * hex values the adherence lint bans, and the brand forbids spring physics —
 * so the frozen tree keeps them and the mobile tree never sees them.
 *
 * Two output *directories*, not two entry files in one. `frontendDist` names a
 * directory and Tauri always serves the `index.html` inside it, so two entries
 * in one folder would load the desktop UI on the phone.
 */
export default defineConfig(({ mode }) => {
  const mobile = mode === "mobile";

  return {
    plugins: [react()],

    // Vite resolves `outDir` relative to `root`, so with a nested mobile root a
    // bare relative path would land inside src/mobile-entry/ and the Tauri
    // overlay would point at nothing. Absolute paths avoid that entirely.
    root: mobile ? resolve(__dirname, "src/mobile-entry") : __dirname,

    build: {
      outDir: resolve(__dirname, mobile ? "dist-mobile" : "dist-desktop"),
      emptyOutDir: true,
    },

    // Prevent Vite from obscuring rust errors.
    clearScreen: false,

    server: {
      // @ts-expect-error process is a nodejs global
      port: parseInt(process.env.PORT || "1420"),
      strictPort: true,
      host: host || false,
      hmr: host
        ? {
            protocol: "ws",
            host,
            // A distinct port from the dev server. Sharing one meant HMR never
            // attached on a physical device.
            port: 1421,
          }
        : undefined,
      watch: {
        ignored: ["**/src-tauri/**"],
      },
    },
  };
});
