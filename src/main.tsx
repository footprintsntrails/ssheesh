import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyAppearance, useSettings } from "./lib/settings";
import { seedDemoLayout } from "./lib/store";
import "./index.css";

applyAppearance(useSettings.getState());
seedDemoLayout();

// The mouse's back and forward buttons move through the Files pane's
// folders; they must never take the window back through its own page history.
window.addEventListener("mouseup", (e) => {
  if (e.button === 3 || e.button === 4) e.preventDefault();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
