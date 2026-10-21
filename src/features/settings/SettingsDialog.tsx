import { useState } from "react";
import { commands, type Settings } from "../../api";
import { Alert, Button, Dialog, Field, Select, TextInput } from "../../components/ui";
import { errorText } from "../../lib/errors";
import { ClaudeIntegration } from "./ClaudeIntegration";

export interface SettingsDialogProps {
  settings: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
}

export function SettingsDialog({ settings, onClose, onSaved }: SettingsDialogProps) {
  const [typingModel, setTypingModel] = useState(settings.typingModel);
  const [expertModel, setExpertModel] = useState(settings.expertModel);
  const [apiKey, setApiKey] = useState("");
  const [workspaceId, setWorkspaceId] = useState(settings.workspaceId);
  const [error, setError] = useState<string | null>(null);

  const modelOptions = settings.models.map((m) => ({ value: m.id, label: m.label }));

  const save = async () => {
    try {
      onSaved(await commands.saveSettings(typingModel, expertModel, apiKey.trim() || null, workspaceId));
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <Dialog
      title="Settings"
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <Field
        label="Claude API key"
        hint="Stored in your system keychain. Only your question and the database schema (table and column names) are sent to Claude, never the data in your tables."
      >
        {(id) => (
          <TextInput
            id={id}
            type="password"
            mono
            value={apiKey}
            placeholder={settings.hasApiKey ? "Saved in Keychain — paste a new key to replace it" : "sk-ant-…"}
            onChange={(e) => setApiKey(e.target.value)}
          />
        )}
      </Field>
      <Field
        label="Workspace ID"
        optional
        hint="Only for API keys that aren't tied to a workspace. Use the workspace's ID from the Claude Console, not its name."
      >
        {(id) => <TextInput id={id} mono value={workspaceId} placeholder="wrkspc_…" onChange={(e) => setWorkspaceId(e.target.value)} />}
      </Field>
      <Field label="Model while typing" hint="Writes SQL as you type. A fast model keeps it feeling instant.">
        {(id) => <Select id={id} value={typingModel} onChange={setTypingModel} options={modelOptions} />}
      </Field>
      <Field
        label="Model for fixes and improvements"
        hint="Fixes a query the database rejected, and reviews a query when you click Improve. A stronger model is worth the extra seconds here."
      >
        {(id) => <Select id={id} value={expertModel} onChange={setExpertModel} options={modelOptions} />}
      </Field>
      <Field label="Use Sayql from Claude (MCP)">{() => <ClaudeIntegration />}</Field>
      {error && <Alert tone="danger">{error}</Alert>}
    </Dialog>
  );
}
