import { useState } from "react";
import type { Settings } from "../../api";

/** Short display names of the two models, e.g. "Haiku 5.5". */
export interface ModelNames {
  typing: string;
  expert: string;
}

function shortName(settings: Settings, id: string): string {
  if (settings.provider === "anthropic") {
    const label = settings.models.find((m) => m.id === id)?.label ?? id;
    return label.split(" — ")[0].replace(/^Claude /, "");
  }
  // "openai/gpt-x" (OpenRouter) and "models/gemini-x" (Gemini) read better without the prefix.
  return id.slice(id.lastIndexOf("/") + 1);
}

/** App settings, initialised from the bootstrap payload and replaced when the dialog saves. */
export function useSettings(initial: Settings) {
  const [settings, setSettings] = useState(initial);
  const modelNames: ModelNames = {
    typing: shortName(settings, settings.typingModel),
    expert: shortName(settings, settings.expertModel),
  };
  return { settings, setSettings, modelNames };
}
