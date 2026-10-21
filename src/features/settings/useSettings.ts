import { useState } from "react";
import type { Settings } from "../../api";

/** App settings, initialised from the bootstrap payload and replaced when the dialog saves. */
export function useSettings(initial: Settings) {
  const [settings, setSettings] = useState(initial);
  const modelName = settings.models.find((m) => m.id === settings.model)?.label.split(" — ")[0] ?? settings.model;
  return { settings, setSettings, modelName };
}
