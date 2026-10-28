import { useEffect, useState } from "react";
import { Check, Copy, X } from "lucide-react";
import { commands } from "../../api";
import { Button, IconButton } from "../../components/ui";
import { cx } from "../../lib/cx";
import { errorText } from "../../lib/errors";
import { auditCommand, copyAuditPrompt, type AuditTarget } from "./audit";
import styles from "./TeachSayql.module.css";

export interface TeachSayqlProps {
  /** Opens the setup: Settings → AI apps → Access → Connect an app. */
  onSetUp?: () => void;
  /** The connection the card is about: the command names it, and Copy audit prompt is offered. */
  target?: AuditTarget | null;
  /** Show the title (a dialog shows it in its own header). */
  heading?: boolean;
  /** Hides the card for good, where it can be hidden. */
  onDismiss?: () => void;
}

/** Whether an AI app has used SayQL over MCP on this computer. */
function useAppConnected(): boolean {
  const [connected, setConnected] = useState(false);
  useEffect(() => {
    let live = true;
    Promise.resolve()
      .then(() => commands.mcpApps())
      .then(
        (list) => live && setConnected(list.apps.some((a) => a.lastUsed != null)),
        () => undefined,
      );
    return () => {
      live = false;
    };
  }, []);
  return connected;
}

/**
 * "Teach SayQL your database": connect a coding agent, run the audit in your project, approve
 * the notes it suggests. Shown on first use and as the empty state of suggested notes.
 */
export function TeachSayql({ onSetUp, target, heading = true, onDismiss }: TeachSayqlProps) {
  const connected = useAppConnected();
  const [copied, setCopied] = useState<"command" | "prompt" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const command = auditCommand(target);
  const flash = (what: "command" | "prompt") => {
    setCopied(what);
    window.setTimeout(() => setCopied(null), 1500);
  };
  const copyCommand = () => {
    setError(null);
    navigator.clipboard.writeText(command).then(() => flash("command"), (e) => setError(errorText(e)));
  };
  const copyPrompt = () => {
    if (!target) return;
    setError(null);
    copyAuditPrompt(target).then(() => flash("prompt"), (e) => setError(errorText(e)));
  };

  return (
    <section className={cx(styles.card, !heading && styles.bare)} aria-label="Teach SayQL your database">
      {(heading || onDismiss) && (
        <div className={styles.head}>
          {heading && <h3 className={styles.title}>Teach SayQL your database</h3>}
          {onDismiss && <IconButton icon={X} label="Don't show again" size="sm" onClick={onDismiss} />}
        </div>
      )}
      <p className={styles.lead}>
        Your code knows things your schema doesn&apos;t: which tables are retired, what a status means. Let your coding
        agent tell SayQL.
      </p>
      <ol className={styles.steps}>
        <li className={connected ? styles.done : undefined}>
          <span className={styles.marker} aria-hidden>
            {connected ? <Check size={12} /> : 1}
          </span>
          <div className={styles.body}>
            {connected ? (
              <>
                <strong>An AI app is connected.</strong> Claude Code, Cursor or another app already uses SayQL.
              </>
            ) : (
              <>
                <strong>Connect Claude Code or Cursor.</strong> For Claude Code it&apos;s one command.
                {onSetUp && (
                  <>
                    {" "}
                    <button type="button" className={styles.link} onClick={onSetUp}>
                      Set it up
                    </button>
                  </>
                )}
              </>
            )}
          </div>
        </li>
        <li>
          <span className={styles.marker} aria-hidden>
            2
          </span>
          <div className={styles.body}>
            <strong>Run an audit in your project.</strong> In Claude Code, type{" "}
            <code className={styles.command}>{command}</code>
            <IconButton
              icon={copied === "command" ? Check : Copy}
              label={copied === "command" ? "Copied" : "Copy the command"}
              size="sm"
              onClick={copyCommand}
            />
            <br />
            It reads your schema, its usage figures and your code, and suggests notes. It needs no rows; any query it runs
            gets only what you allow on that connection.
            {target ? (
              <div className={styles.copy}>
                <Button size="sm" variant="ghost" icon={copied === "prompt" ? Check : Copy} onClick={copyPrompt}>
                  {copied === "prompt" ? "Copied" : "Copy audit prompt"}
                </Button>
                <span className={styles.hint}>for apps without MCP prompts: paste it into the chat.</span>
              </div>
            ) : (
              <span className={styles.hint}>
                {" "}
                App without MCP prompts? Copy the audit prompt for a connection in Settings → AI apps → Access.
              </span>
            )}
          </div>
        </li>
        <li>
          <span className={styles.marker} aria-hidden>
            3
          </span>
          <div className={styles.body}>
            <strong>Approve what&apos;s right.</strong> Every note you accept makes SayQL&apos;s answers better, for you and
            every agent after.
          </div>
        </li>
      </ol>
      <p className={styles.again}>
        Run it again now and then, and after big changes: code moves on and notes go stale. The audit also flags notes that
        look out of date.
      </p>
      {error && <p className={styles.error}>{error}</p>}
    </section>
  );
}
