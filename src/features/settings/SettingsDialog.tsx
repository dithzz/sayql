import { useId, useState } from "react";
import { RefreshCw } from "lucide-react";
import { commands, type AiProvider, type Settings } from "../../api";
import { Alert, Button, Dialog, Field, Select, TextInput } from "../../components/ui";
import { errorText } from "../../lib/errors";
import { SkillsSection, type SkillsApi } from "../skills";
import { ClaudeIntegration } from "./ClaudeIntegration";
import { version } from "../../../package.json";
import styles from "./SettingsDialog.module.css";

export interface SettingsDialogProps {
  settings: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
  /** Reopens the first-run setup. */
  onShowSetup: () => void;
  skills: SkillsApi;
  /** Opened from "Create skill…" in the Ask box: start a new skill. */
  createSkill?: boolean;
}

/** What the form holds for one provider; kept per provider so switching back loses nothing. */
interface Draft {
  baseUrl: string;
  typingModel: string;
  expertModel: string;
  apiKey: string;
}

const draftOf = (p: AiProvider): Draft => ({ baseUrl: p.baseUrl, typingModel: p.typingModel, expertModel: p.expertModel, apiKey: "" });

/** Where the question and schema go, in one sentence. Local providers say so in their note. */
function privacyNote(p: AiProvider): string {
  const to = p.key === "optional" ? "your server" : p.label;
  return `Your question and table and column names are sent to ${to}, never the data in your tables.`;
}

export function SettingsDialog({ settings, onClose, onSaved, onShowSetup, skills, createSkill }: SettingsDialogProps) {
  const [providerId, setProviderId] = useState(settings.provider);
  const [drafts, setDrafts] = useState<Record<string, Draft>>(() =>
    Object.fromEntries(settings.providers.map((p) => [p.id, draftOf(p)])),
  );
  const [workspaceId, setWorkspaceId] = useState(settings.workspaceId);
  const [models, setModels] = useState<Record<string, string[]>>({});
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const listId = useId();

  const provider = settings.providers.find((p) => p.id === providerId) ?? settings.providers[0];
  const draft = drafts[provider.id];
  const update = (patch: Partial<Draft>) => setDrafts((d) => ({ ...d, [provider.id]: { ...d[provider.id], ...patch } }));
  const claudeModels = settings.models.map((m) => ({ value: m.id, label: m.label }));
  const found = models[provider.id];

  const chooseProvider = (id: string) => {
    setProviderId(id);
    setModelsError(null);
    setError(null);
  };

  const loadModels = async () => {
    setLoadingModels(true);
    setModelsError(null);
    try {
      const ids = await commands.listModels(provider.id, draft.baseUrl, draft.apiKey.trim() || null);
      setModels((m) => ({ ...m, [provider.id]: ids }));
      if (!draft.typingModel && ids.length > 0) update({ typingModel: ids[0] });
    } catch (e) {
      setModelsError(errorText(e));
    } finally {
      setLoadingModels(false);
    }
  };

  const save = async () => {
    try {
      onSaved(
        await commands.saveSettings({
          provider: provider.id,
          typingModel: draft.typingModel,
          expertModel: draft.expertModel,
          baseUrl: draft.baseUrl,
          apiKey: draft.apiKey.trim() || null,
          workspaceId,
        }),
      );
    } catch (e) {
      setError(errorText(e));
    }
  };

  const modelHint = found
    ? `${found.length} model${found.length === 1 ? "" : "s"} available. Type to filter, or enter any model id.`
    : "Enter a model id, or load the list from the provider.";

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
      <Field label="AI provider" hint={provider.note}>
        {(id) => (
          <Select
            id={id}
            value={provider.id}
            onChange={chooseProvider}
            options={settings.providers.map((p) => ({ value: p.id, label: p.local ? `${p.label} (on this computer)` : p.label }))}
          />
        )}
      </Field>

      {provider.key !== "none" && (
        <Field label={`${provider.label} API key`} optional={provider.key === "optional"} hint={`Stored in your system keychain. ${privacyNote(provider)}`}>
          {(id) => (
            <TextInput
              id={id}
              type="password"
              mono
              value={draft.apiKey}
              placeholder={provider.hasKey ? "Saved in Keychain — paste a new key to replace it" : provider.keyPlaceholder || "Paste your API key"}
              onChange={(e) => update({ apiKey: e.target.value })}
            />
          )}
        </Field>
      )}

      {provider.api === "anthropic" ? (
        <>
          <Field
            label="Workspace ID"
            optional
            hint="Only for API keys that aren't tied to a workspace. Use the workspace's ID from the Claude Console, not its name."
          >
            {(id) => <TextInput id={id} mono value={workspaceId} placeholder="wrkspc_…" onChange={(e) => setWorkspaceId(e.target.value)} />}
          </Field>
          <Field label="Model while typing" hint="Writes SQL as you type. A fast model keeps it feeling instant.">
            {(id) => <Select id={id} value={draft.typingModel} onChange={(v) => update({ typingModel: v })} options={claudeModels} />}
          </Field>
          <Field
            label="Model for fixes and improvements"
            hint="Fixes a query the database rejected, and reviews a query when you click Improve. A stronger model is worth the extra seconds here."
          >
            {(id) => <Select id={id} value={draft.expertModel} onChange={(v) => update({ expertModel: v })} options={claudeModels} />}
          </Field>
        </>
      ) : (
        <>
          <Field
            label="Base URL"
            optional={!!provider.defaultBaseUrl}
            hint={
              provider.local
                ? `Where ${provider.label} listens. Change it only if you run it on another port or machine.`
                : "The address of the OpenAI-compatible API, usually ending in /v1."
            }
          >
            {(id) => (
              <TextInput
                id={id}
                mono
                value={draft.baseUrl}
                placeholder={provider.defaultBaseUrl || "https://your-server.example.com/v1"}
                onChange={(e) => update({ baseUrl: e.target.value })}
              />
            )}
          </Field>
          <Field label="Model while typing" hint={modelHint}>
            {(id) => (
              <div className={styles.inline}>
                <TextInput
                  id={id}
                  mono
                  list={listId}
                  value={draft.typingModel}
                  placeholder="Model id"
                  onChange={(e) => update({ typingModel: e.target.value })}
                />
                <Button icon={RefreshCw} loading={loadingModels} onClick={loadModels}>
                  Load models
                </Button>
              </div>
            )}
          </Field>
          <Field
            label="Model for fixes and improvements"
            optional
            hint="A stronger model for fixing rejected queries and for Improve. Leave empty to use the model above."
          >
            {(id) => (
              <TextInput
                id={id}
                mono
                list={listId}
                value={draft.expertModel}
                placeholder="Same as above"
                onChange={(e) => update({ expertModel: e.target.value })}
              />
            )}
          </Field>
          <datalist id={listId}>
            {found?.map((m) => <option key={m} value={m} />)}
          </datalist>
          {modelsError && <Alert tone="danger">{modelsError}</Alert>}
        </>
      )}

      <Field label="Skills" hint="Saved instructions you add to a question by typing / and the name. Saved right away.">
        {() => <SkillsSection skills={skills} startCreating={createSkill} />}
      </Field>
      <Field label="Use SayQL from AI apps (MCP)">{() => <ClaudeIntegration />}</Field>
      <Field label="About">
        {() => (
          <p className={styles.about}>
            SayQL Community Edition {version}. Free and open source under the Apache License 2.0. The license, privacy
            notice and third-party notices are included with the app.{" "}
            <button type="button" className={styles.link} onClick={onShowSetup}>
              Show the setup guide again
            </button>
          </p>
        )}
      </Field>
      {error && <Alert tone="danger">{error}</Alert>}
    </Dialog>
  );
}
