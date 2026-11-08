import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { engine } from "../audio/AudioEngine.ts";
import { inPlugin } from "../audio/pluginBridge.ts";

export async function mount(root: HTMLElement): Promise<void> {
  // the stores start from the plugin's state, or a first render would show (and a control could send) defaults
  if (inPlugin) await (await import("./pluginState.ts")).attachToPlugin();
  // console/debug handle (used by the browser smoke test)
  (window as unknown as { __chimeraEngine: typeof engine }).__chimeraEngine = engine;
  root.replaceChildren();
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
