import { useState, type ReactNode } from "react";
import { ArrowRight, Check, Database, HardDrive, MessageSquareText, Plug, ShieldCheck, Sparkles } from "lucide-react";
import { commands, type AiProvider, type Settings } from "../../api";
import { Alert, BrandMark, Button, Dialog, Field, Select, TextInput } from "../../components/ui";
import { cx } from "../../lib/cx";
import { errorText } from "../../lib/errors";
import styles from "./OnboardingDialog.module.css";

export type OnboardingStep = "welcome" | "ai" | "database" | "try";
const STEPS: OnboardingStep[] = ["welcome", "ai", "database", "try"];

/** Shown as cards; every other provider is in the "More providers" list. */
const FEATURED = ["anthropic", "openai", "gemini", "ollama"];

export interface OnboardingDialogProps {
  settings: Settings;
  step: OnboardingStep;
  onStep: (step: OnboardingStep) => void;
  onSettingsSaved: (s: Settings) => void;
  /** False when the person deleted the demo connection. */
  demoAvailable: boolean;
  onUseDemo: () => void;
  onConnectOwn: () => void;
  /** Starter questions for the database picked in the previous step. */
  examples: string[];
  /** Opens a tab that asks `question` and runs the answer; ends the setup. */
  onTry: (question: string) => void;
  onOpenSettings: () => void;
  /** Ends the setup (finished, skipped or closed). */
  onFinish: () => void;
}

/** First-run setup: what SayQL is, pick an AI, pick a database, ask a first question. */
export function OnboardingDialog(p: OnboardingDialogProps) {
  const index = STEPS.indexOf(p.step);
  const title = { welcome: "Welcome to SayQL", ai: "Pick your AI", database: "Pick a database", try: "Ask your first question" }[p.step];

  return (
    <Dialog title={title} onClose={p.onFinish} actions={null}>
      <div className={styles.progress} aria-label={`Step ${index + 1} of ${STEPS.length}`}>
        {STEPS.map((s, i) => (
          <span key={s} className={cx(styles.dot, i <= index && styles.dotDone)} />
        ))}
      </div>
      {p.step === "welcome" && <Welcome {...p} />}
      {p.step === "ai" && <ChooseAi {...p} />}
      {p.step === "database" && <ChooseDatabase {...p} />}
      {p.step === "try" && <TryIt {...p} />}
    </Dialog>
  );
}

function Footer({ children }: { children: ReactNode }) {
  return <div className={styles.footer}>{children}</div>;
}

function Welcome(p: OnboardingDialogProps) {
  const points = [
    { icon: MessageSquareText, text: "Type what you need in plain English. SayQL writes the SQL as you type." },
    { icon: ShieldCheck, text: "Queries run read-only. Anything that could change data asks you first." },
    { icon: HardDrive, text: "The Ask box never sends your rows to the AI: only your question and table and column names." },
  ];
  return (
    <>
      <div className={styles.hero}>
        <BrandMark size={52} />
        <p className={styles.lede}>Ask your database in plain English. Setup takes about a minute.</p>
      </div>
      <ul className={styles.points}>
        {points.map(({ icon: Icon, text }) => (
          <li key={text}>
            <span className={styles.pointIcon}>
              <Icon size={15} />
            </span>
            {text}
          </li>
        ))}
      </ul>
      <Footer>
        <Button variant="ghost" onClick={p.onFinish}>
          Set up later
        </Button>
        <Button variant="primary" icon={ArrowRight} onClick={() => p.onStep("ai")}>
          Get started
        </Button>
      </Footer>
    </>
  );
}

function ChooseAi(p: OnboardingDialogProps) {
  const { settings } = p;
  const [providerId, setProviderId] = useState(settings.provider);
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [models, setModels] = useState<string[] | null>(null);
  const [model, setModel] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const provider = settings.providers.find((x) => x.id === providerId) ?? settings.providers[0];
  const featured = FEATURED.map((id) => settings.providers.find((x) => x.id === id)).filter(Boolean) as AiProvider[];
  const more = settings.providers.filter((x) => !FEATURED.includes(x.id));
  const isClaude = provider.api === "anthropic";

  const choose = (id: string) => {
    setProviderId(id);
    setApiKey("");
    setBaseUrl("");
    setModels(null);
    setModel("");
    setError(null);
  };

  /** Checks the key (and address), then saves; other providers first pick a model from the list. */
  const next = async () => {
    setBusy(true);
    setError(null);
    try {
      if (isClaude || !models) {
        const ids = await commands.listModels(provider.id, baseUrl, apiKey.trim() || null);
        if (!isClaude) {
          // Pick (or type, when the list is empty) the model first; Continue then saves.
          setModels(ids);
          setModel(provider.typingModel || ids[0] || "");
          return;
        }
      }
      const chosen = isClaude ? provider : null;
      const saved = await commands.saveSettings({
        provider: provider.id,
        typingModel: chosen ? chosen.typingModel : model.trim(),
        expertModel: chosen ? chosen.expertModel : "",
        baseUrl,
        apiKey: apiKey.trim() || null,
        workspaceId: settings.workspaceId,
      });
      p.onSettingsSaved(saved);
      p.onStep("database");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const needsKey = provider.key === "required" && !provider.hasKey && !apiKey.trim();
  const needsUrl = !provider.defaultBaseUrl && !baseUrl.trim();
  const needsModel = !!models && !model.trim();

  return (
    <>
      <p className={styles.intro}>SayQL works with the AI you prefer. You can change this any time in Settings.</p>
      <div className={styles.cards} role="radiogroup" aria-label="AI provider">
        {featured.map((x) => (
          <button
            key={x.id}
            type="button"
            role="radio"
            aria-checked={x.id === provider.id}
            className={cx(styles.card, x.id === provider.id && styles.cardOn)}
            onClick={() => choose(x.id)}
          >
            <span className={styles.cardTitle}>
              {x.label}
              {x.id === "anthropic" && <span className={styles.tag}>Recommended</span>}
              {x.local && <span className={cx(styles.tag, styles.tagLocal)}>On this computer</span>}
            </span>
            <span className={styles.cardNote}>{x.note}</span>
          </button>
        ))}
      </div>
      <Field label="More providers">
        {(id) => (
          <Select
            id={id}
            value={FEATURED.includes(provider.id) ? "" : provider.id}
            onChange={(v) => v && choose(v)}
            options={[{ value: "", label: "OpenRouter, Mistral, Groq, DeepSeek, LM Studio, other…" }, ...more.map((x) => ({ value: x.id, label: x.label }))]}
          />
        )}
      </Field>

      {provider.key !== "none" && (
        <Field
          label={`${provider.label} API key`}
          optional={provider.key === "optional"}
          hint="Stored in your system keychain, never in a file."
        >
          {(id) => (
            <TextInput
              id={id}
              type="password"
              mono
              autoFocus
              value={apiKey}
              placeholder={provider.hasKey ? "Saved in Keychain — paste a new key to replace it" : provider.keyPlaceholder || "Paste your API key"}
              onChange={(e) => {
                setApiKey(e.target.value);
                setModels(null);
              }}
            />
          )}
        </Field>
      )}
      {(!provider.defaultBaseUrl || provider.local) && (
        <Field label="Base URL" optional={!!provider.defaultBaseUrl}>
          {(id) => (
            <TextInput
              id={id}
              mono
              value={baseUrl}
              placeholder={provider.defaultBaseUrl || "https://your-server.example.com/v1"}
              onChange={(e) => {
                setBaseUrl(e.target.value);
                setModels(null);
              }}
            />
          )}
        </Field>
      )}
      {models && (
        <Field label="Model" hint="A fast model keeps SQL appearing as you type. You can add a stronger one for fixes in Settings.">
          {(id) =>
            models.length ? (
              <Select id={id} value={model} onChange={setModel} options={models.map((m) => ({ value: m, label: m }))} />
            ) : (
              <TextInput id={id} mono value={model} placeholder="Model id" onChange={(e) => setModel(e.target.value)} />
            )
          }
        </Field>
      )}
      {error && <Alert tone="danger">{error}</Alert>}

      <Footer>
        <Button variant="ghost" onClick={() => p.onStep("database")}>
          Skip for now
        </Button>
        <Button variant="primary" icon={models ? Check : Plug} loading={busy} disabled={needsKey || needsUrl || needsModel} onClick={next}>
          {models ? "Continue" : isClaude ? "Check key and continue" : "Connect"}
        </Button>
      </Footer>
    </>
  );
}

function ChooseDatabase(p: OnboardingDialogProps) {
  return (
    <>
      <p className={styles.intro}>Start with the built-in demo, or connect a database of your own. You can add more later.</p>
      <div className={styles.choices}>
        {p.demoAvailable && (
          <button type="button" className={styles.choice} onClick={p.onUseDemo}>
            <span className={styles.choiceIcon}>
              <Sparkles size={18} />
            </span>
            <span>
              <b>Try the demo database</b>
              <span>A small shop with customers, orders and products. Nothing to set up.</span>
            </span>
            <ArrowRight size={16} className={styles.choiceArrow} />
          </button>
        )}
        <button type="button" className={styles.choice} onClick={p.onConnectOwn}>
          <span className={styles.choiceIcon}>
            <Database size={18} />
          </span>
          <span>
            <b>Connect my own database</b>
            <span>PostgreSQL, MySQL, SQLite, SQL Server or Azure SQL.</span>
          </span>
          <ArrowRight size={16} className={styles.choiceArrow} />
        </button>
      </div>
      <Footer>
        <Button variant="ghost" onClick={() => p.onStep("ai")}>
          Back
        </Button>
        <Button variant="ghost" onClick={p.onFinish}>
          Skip
        </Button>
      </Footer>
    </>
  );
}

function TryIt(p: OnboardingDialogProps) {
  if (!p.settings.aiReady) {
    return (
      <>
        <Alert tone="warning">AI is off, so SayQL can't write SQL yet. You can still browse tables and run your own SQL.</Alert>
        <p className={styles.intro}>Add an AI provider any time in Settings to turn plain English into SQL.</p>
        <Footer>
          <Button
            variant="ghost"
            onClick={() => {
              p.onFinish();
              p.onOpenSettings();
            }}
          >
            Open Settings
          </Button>
          <Button variant="primary" onClick={p.onFinish}>
            Start using SayQL
          </Button>
        </Footer>
      </>
    );
  }
  return (
    <>
      <p className={styles.intro}>Pick one to see SayQL write the SQL and run it. Or close this and type your own question in the Ask box.</p>
      <div className={styles.choices}>
        {p.examples.map((q) => (
          <button key={q} type="button" className={cx(styles.choice, styles.example)} onClick={() => p.onTry(q)}>
            <span className={styles.choiceIcon}>
              <MessageSquareText size={16} />
            </span>
            <span>
              <b>{q}</b>
            </span>
            <ArrowRight size={16} className={styles.choiceArrow} />
          </button>
        ))}
        {p.examples.length === 0 && <p className={styles.intro}>Type a question in the Ask box above the editor to get started.</p>}
      </div>
      <Footer>
        <Button variant="primary" onClick={p.onFinish}>
          I'll type my own
        </Button>
      </Footer>
    </>
  );
}
