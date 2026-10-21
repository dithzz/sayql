import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";
import "./styles/base.css";

async function start() {
  // In a plain browser during development, fake the Rust backend so the UI can be previewed.
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    (await import("./dev/mockBackend")).installMockBackend();
  }
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void start();
