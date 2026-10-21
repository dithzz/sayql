import { forwardRef, useImperativeHandle, useLayoutEffect, useRef } from "react";
import { CheckCircle2, CornerDownRight, Sparkles, XCircle, Zap } from "lucide-react";
import { Button, Kbd, Spinner } from "../../components/ui";
import { cx } from "../../lib/cx";
import type { GenerationStatus } from "./useSqlGeneration";
import styles from "./AskBar.module.css";

export interface AskBarProps {
  question: string;
  onQuestionChange: (q: string) => void;
  onSubmit: () => void;
  status: GenerationStatus;
  modelName: string;
  disabled: boolean;
  /** Clickable example questions shown while the box is empty. */
  examples?: string[];
  onOpenSettings: () => void;
  onRetry: () => void;
  /** Present when requests build on the last run query. */
  followUp?: { onStartFresh: () => void };
}

/** The plain-English input. SQL appears in the editor below as the person types. */
export const AskBar = forwardRef<HTMLTextAreaElement, AskBarProps>(function AskBar(p, ref) {
  const input = useRef<HTMLTextAreaElement>(null);
  useImperativeHandle(ref, () => input.current!, []);

  // Grow with the text, up to the CSS max-height.
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [p.question]);

  return (
    <div className={styles.ask}>
      <div className={styles.inputWrap}>
        <span className={styles.sparkle}>
          <Sparkles size={16} />
        </span>
        <textarea
          ref={input}
          className={styles.input}
          rows={1}
          value={p.question}
          disabled={p.disabled}
          aria-label="Ask in plain English"
          placeholder="Ask in plain English — e.g. top 10 customers by total spend this year"
          onChange={(e) => p.onQuestionChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              p.onSubmit();
            } else if (e.key === "Escape") {
              p.onQuestionChange("");
            }
          }}
        />
      </div>
      <div className={styles.footer}>
        {p.followUp && (
          <span className={styles.followUp} title="Requests like “remove a column” or “only active ones” change the last query you ran">
            <CornerDownRight size={13} /> Building on your last query
            <Button size="sm" variant="ghost" onClick={p.followUp.onStartFresh}>
              New query
            </Button>
          </span>
        )}
        <StatusLine status={p.status} modelName={p.modelName} onOpenSettings={p.onOpenSettings} onRetry={p.onRetry} />
        {!p.question && p.examples?.length ? (
          <div className={styles.examples}>
            {p.examples.map((ex) => (
              <button
                key={ex}
                type="button"
                className={styles.example}
                onClick={() => {
                  p.onQuestionChange(ex);
                  input.current?.focus(); // so Enter runs it
                }}
              >
                {ex}
              </button>
            ))}
          </div>
        ) : (
          <span className={styles.hints}>
            <Kbd>↵</Kbd> run <Kbd>⇧↵</Kbd> new line <Kbd>esc</Kbd> clear
          </span>
        )}
      </div>
    </div>
  );
});

function StatusLine({
  status,
  modelName,
  onOpenSettings,
  onRetry,
}: {
  status: GenerationStatus;
  modelName: string;
  onOpenSettings: () => void;
  onRetry: () => void;
}) {
  switch (status.kind) {
    case "idle":
      return <span className={styles.status}>SQL appears in the editor as you type.</span>;
    case "instant":
      return (
        <span className={cx(styles.status, styles.instant)}>
          <Zap size={13} /> Instant · {status.source === "rules" ? "simple request, no AI needed" : "same question as before"}
        </span>
      );
    case "writing":
      return (
        <span className={cx(styles.status, styles.writing)}>
          <Spinner size={13} /> Writing SQL with {modelName}…
        </span>
      );
    case "done":
      return (
        <span className={cx(styles.status, styles.done)}>
          <CheckCircle2 size={13} /> {modelName}
          {status.firstTokenMs != null && ` · first SQL in ${status.firstTokenMs} ms`} · {status.totalMs} ms total
        </span>
      );
    case "error":
      return (
        <span className={cx(styles.status, styles.error)}>
          <XCircle size={13} /> {status.message}
          {status.fixInSettings && (
            <Button size="sm" variant="ghost" onClick={onOpenSettings}>
              Open Settings
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={onRetry}>
            Retry
          </Button>
        </span>
      );
  }
}
