import { Check, ChevronDown, Settings as SettingsIcon } from "lucide-react";
import { commands, type AiProvider, type Settings } from "../../api";
import { ContextMenu, useContextMenu, type MenuEntry } from "../../components/ui";
import { errorText } from "../../lib/errors";
import { modelLabel } from "./useSettings";
import styles from "./ModelPicker.module.css";

export interface ModelPickerProps {
  settings: Settings;
  /** Quick mode sets the model that writes SQL as you type; agent mode the one that runs the agent. */
  purpose: "typing" | "agent";
  onChange: (s: Settings) => void;
  onError: (message: string) => void;
  onOpenSettings: () => void;
}

/** The models a provider offers here: Claude's list, or the ones chosen for it in Settings. */
function modelsOf(settings: Settings, p: AiProvider): string[] {
  if (p.api === "anthropic") return settings.models.map((m) => m.id);
  return [...new Set([p.typingModel, p.expertModel, p.agentModel].filter(Boolean))];
}

/** The model in use for `purpose`, as an id. */
export function currentModel(settings: Settings, purpose: "typing" | "agent"): string {
  if (purpose === "typing") return settings.typingModel;
  return settings.agentModel || settings.expertModel;
}

/** A small button in the Ask box that switches the model, and the provider, without opening Settings. */
export function ModelPicker({ settings, purpose, onChange, onError, onOpenSettings }: ModelPickerProps) {
  const menu = useContextMenu();
  const current = currentModel(settings, purpose);
  const ready = settings.providers.filter((p) => p.ready);

  const choose = (provider: string, model: string) =>
    commands.chooseModel(provider, model, purpose).then(onChange, (e) => onError(errorText(e)));

  const entries = (p: AiProvider): MenuEntry[] =>
    modelsOf(settings, p).map((id) => ({
      label: modelLabel(settings, id, p.id),
      icon: p.id === settings.provider && id === current ? Check : undefined,
      onSelect: () => void choose(p.id, id),
    }));

  const items: MenuEntry[] = [
    ...(ready.length === 1
      ? entries(ready[0])
      : ready.map((p): MenuEntry => ({ label: p.id === settings.provider ? `${p.label} (in use)` : p.label, submenu: entries(p) }))),
    "separator",
    { label: "More models and providers…", icon: SettingsIcon, onSelect: onOpenSettings },
  ];

  if (!settings.aiReady) return null;
  return (
    <>
      <button
        type="button"
        className={styles.picker}
        title={purpose === "agent" ? "The model that runs agent mode. Click to switch." : "The model that writes SQL as you type. Click to switch."}
        onClick={(e) => menu.open(e, items)}
      >
        {modelLabel(settings, current)}
        <ChevronDown size={12} />
      </button>
      {menu.menu && <ContextMenu at={menu.menu.at} items={menu.menu.items} onClose={menu.close} />}
    </>
  );
}
