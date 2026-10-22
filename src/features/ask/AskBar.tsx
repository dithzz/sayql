import { forwardRef, useEffect, useId, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { BookOpen, CheckCircle2, CornerDownRight, Sparkles, X, XCircle, Zap } from "lucide-react";
import type { Skill } from "../../api";
import { Button, IconButton, Kbd, Spinner } from "../../components/ui";
import { cx } from "../../lib/cx";
import type { ModelNames } from "../settings";
import type { GenerationStatus } from "./useSqlGeneration";
import { ContextPill } from "./ContextPill";
import { SlashMenu, itemId } from "./SlashMenu";
import { commandSlug, isAction, menuItems, menuQuery, parseCommand, withCommand, type MenuItem } from "./slashCommands";
import styles from "./AskBar.module.css";

export interface AskBarProps {
  question: string;
  onQuestionChange: (q: string) => void;
  onSubmit: () => void;
  status: GenerationStatus;
  modelNames: ModelNames;
  disabled: boolean;
  onOpenSettings: () => void;
  onRetry: () => void;
  /** Tables picked in the navigator as context for the AI. */
  focusTables: string[];
  /** Whether the picked tables go to the AI (the pill's eye toggle). */
  focusEnabled: boolean;
  onFocusEnabledChange: (enabled: boolean) => void;
  onFocusTablesChange: (names: string[]) => void;
  /** Present when requests build on the last run query. */
  followUp?: { onStartFresh: () => void };
  /** Saved skills, offered in the "/" menu. */
  skills: Skill[];
  /** "Create skill…" in the "/" menu. */
  onCreateSkill: () => void;
  /** The /explain answer, while there is one. */
  explanation: string | null;
  onDismissExplanation: () => void;
}

/** The plain-English input. SQL appears in the editor below as the person types. */
export const AskBar = forwardRef<HTMLTextAreaElement, AskBarProps>(function AskBar(p, ref) {
  const input = useRef<HTMLTextAreaElement>(null);
  useImperativeHandle(ref, () => input.current!, []);

  const box = useRef<HTMLDivElement>(null);
  const menuId = useId();

  // A picked command shows as a chip; the text box holds the rest of the question.
  const { command, rest } = parseCommand(p.question, p.skills);
  const slug = command && commandSlug(command);
  const setRest = (text: string) => p.onQuestionChange(slug ? withCommand(slug, text) : text);

  // The "/" menu: open while the question is "/" plus a partial name, until picked or esc
  // (which keeps it closed until the next keystroke).
  const query = command ? null : menuQuery(p.question);
  const [closedAt, setClosedAt] = useState<string | null>(null);
  const items = query === null ? [] : menuItems(query, p.skills);
  const menuOpen = query !== null && closedAt !== p.question;
  const [active, setActive] = useState(0);
  useEffect(() => setActive(0), [query]);

  const pick = (item: MenuItem) => {
    if (item.kind === "create") {
      p.onQuestionChange("");
      p.onCreateSkill();
    } else {
      p.onQuestionChange(withCommand(item.slug, ""));
    }
    input.current?.focus();
  };

  // Grow with the text, up to the CSS max-height.
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [p.question]);

  const placeholder =
    command?.kind === "builtin"
      ? command.builtin.placeholder
      : command
        ? "Ask in plain English"
        : "Ask in plain English — e.g. top 10 customers by total spend this year. Type / for commands";
  // /new and the actions don't build on the last query; /followup always does.
  const followUp = p.followUp && slug !== "new" && !isAction(command) ? p.followUp : undefined;

  return (
    <div className={styles.ask}>
      <div ref={box} className={styles.inputWrap}>
        {p.focusTables.length > 0 && (
          <div className={styles.context}>
            <ContextPill
              tables={p.focusTables}
              enabled={p.focusEnabled}
              onEnabledChange={p.onFocusEnabledChange}
              onClear={() => p.onFocusTablesChange([])}
            />
          </div>
        )}
        <div className={styles.inputRow}>
        <span className={styles.sparkle}>
          <Sparkles size={14} />
        </span>
        {slug && (
          <span className={cx(styles.chip, command?.kind === "skill" && styles.skillChip)} title={command?.kind === "skill" ? command.skill.instruction : command?.builtin.description}>
            /{slug}
            <button type="button" className={styles.chipRemove} aria-label={`Remove /${slug}`} onClick={() => p.onQuestionChange(rest)}>
              <X size={11} />
            </button>
          </span>
        )}
        <textarea
          ref={input}
          className={styles.input}
          rows={1}
          value={rest}
          disabled={p.disabled}
          aria-label="Ask in plain English"
          placeholder={placeholder}
          role="combobox"
          aria-expanded={menuOpen}
          aria-controls={menuOpen ? menuId : undefined}
          aria-autocomplete="list"
          aria-activedescendant={menuOpen ? itemId(menuId, active) : undefined}
          onChange={(e) => {
            setClosedAt(null);
            setRest(e.target.value);
          }}
          onKeyDown={(e) => {
            if (menuOpen) {
              const handled = ["ArrowDown", "ArrowUp", "Enter", "Tab", "Escape"].includes(e.key) && !e.shiftKey;
              if (e.key === "ArrowDown") setActive((active + 1) % items.length);
              else if (e.key === "ArrowUp") setActive((active - 1 + items.length) % items.length);
              else if (e.key === "Enter" || e.key === "Tab") pick(items[active]);
              else if (e.key === "Escape") setClosedAt(p.question);
              if (handled) {
                e.preventDefault();
                return;
              }
            }
            const el = e.currentTarget;
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              p.onSubmit();
            } else if (e.key === "Escape") {
              p.onQuestionChange("");
            } else if (e.key === "Backspace" && slug && el.selectionStart === 0 && el.selectionEnd === 0) {
              // Backspace at the start removes the chip.
              e.preventDefault();
              p.onQuestionChange(rest);
            }
          }}
        />
        </div>
      </div>
      {menuOpen && <SlashMenu id={menuId} items={items} active={active} anchor={box} onActiveChange={setActive} onPick={pick} />}
      {p.explanation !== null && (
        <div className={styles.explanation} role="status">
          <BookOpen size={13} className={styles.explanationIcon} />
          <p className={styles.explanationText}>{p.explanation || "…"}</p>
          <IconButton icon={X} label="Close explanation" size="sm" onClick={p.onDismissExplanation} />
        </div>
      )}
      <div className={styles.footer}>
        {followUp && (
          <span className={styles.followUp} title="Requests like “remove a column” or “only active ones” change the last query you ran">
            <CornerDownRight size={13} /> {slug === "followup" ? "Changing your last query" : "Building on your last query"}
            <Button size="sm" variant="ghost" onClick={followUp.onStartFresh}>
              New query
            </Button>
          </span>
        )}
        <StatusLine status={p.status} modelNames={p.modelNames} onOpenSettings={p.onOpenSettings} onRetry={p.onRetry} />
        <span className={styles.hints}>
          <Kbd>/</Kbd> commands <Kbd>↵</Kbd> {isAction(command) ? slug : "run"} <Kbd>⇧↵</Kbd> new line <Kbd>esc</Kbd> clear
        </span>
      </div>
    </div>
  );
});

function StatusLine({
  status,
  modelNames,
  onOpenSettings,
  onRetry,
}: {
  status: GenerationStatus;
  modelNames: ModelNames;
  onOpenSettings: () => void;
  onRetry: () => void;
}) {
  const writing = {
    draft: `Writing SQL with ${modelNames.typing}…`,
    fix: `Fixing the query with ${modelNames.expert}…`,
    improve: `Reviewing the query with ${modelNames.expert}…`,
    explain: `Explaining the query with ${modelNames.typing}…`,
  };
  const finished = {
    draft: modelNames.typing,
    fix: `Fixed by ${modelNames.expert}`,
    improve: `Reviewed by ${modelNames.expert}`,
    explain: `Explained by ${modelNames.typing}`,
  };
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
          <Spinner size={13} /> {writing[status.task]}
        </span>
      );
    case "done":
      return (
        <span className={cx(styles.status, styles.done)}>
          <CheckCircle2 size={13} /> {finished[status.task]}
          {status.firstTokenMs != null && status.task !== "explain" && ` · first SQL in ${status.firstTokenMs} ms`} · {status.totalMs} ms total
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
