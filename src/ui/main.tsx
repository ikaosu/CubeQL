import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { tauriApi } from "./tauri-api";
import { initEngine } from "./cube-engine";
import "./styles.css";

if (typeof window !== "undefined" && !window.api) {
  window.api = tauriApi;
}

const container = document.getElementById("root");
if (!container) throw new Error("missing #root");

// Load the cube-core WASM module (single source of truth for facelet / move /
// mask / DSL logic) BEFORE rendering React. App.tsx and friends call the
// engine synchronously and would throw if the module weren't ready yet.
initEngine()
  .then(() => {
    createRoot(container).render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
  })
  .catch((err: unknown) => {
    container.textContent = `Failed to load cube-core engine: ${err instanceof Error ? err.message : String(err)}`;
  });
