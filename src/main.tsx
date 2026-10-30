import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyAppearance, useSettings } from "./lib/settings";
import { seedDemoLayout } from "./lib/store";
import "./index.css";

applyAppearance(useSettings.getState());
seedDemoLayout();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
