// Slash commands in the Ask box: "/explain", "/new show customers", "/pii-safe list customers".
// The command stays in the question text as a "/slug " prefix, so tabs save and restore it as is.
import type { GenerationTask, PreviousQuery, Skill } from "../../api";

/** Commands that do something with the current query when the person presses ↵. */
export type ActionCommand = "explain" | "fix" | "improve";
export type BuiltinCommand = "followup" | "new" | ActionCommand;

export interface Builtin {
  slug: BuiltinCommand;
  description: string;
  /** Placeholder for the rest of the question once picked. */
  placeholder: string;
}

/** Must match BUILTIN_COMMANDS in src-tauri/src/commands/skills.rs. */
export const BUILTINS: Builtin[] = [
  { slug: "followup", description: "Change the current query", placeholder: "What to change — e.g. only active ones" },
  { slug: "new", description: "Start fresh, ignore the last query", placeholder: "Ask in plain English" },
  { slug: "explain", description: "Explain the current query", placeholder: "Anything to focus on? (optional)" },
  { slug: "fix", description: "Fix the current query or its error", placeholder: "What's wrong? (optional)" },
  { slug: "improve", description: "Review and improve the current query", placeholder: "Anything to focus on? (optional)" },
];

const ACTIONS: readonly string[] = ["explain", "fix", "improve"] satisfies ActionCommand[];

export type Command = { kind: "builtin"; builtin: Builtin } | { kind: "skill"; skill: Skill };

export const commandSlug = (c: Command) => (c.kind === "builtin" ? c.builtin.slug : c.skill.slug);

export const isAction = (c: Command | null): c is { kind: "builtin"; builtin: Builtin & { slug: ActionCommand } } =>
  c?.kind === "builtin" && ACTIONS.includes(c.builtin.slug);

export interface Parsed {
  command: Command | null;
  /** The question without the command. */
  rest: string;
}

/**
 * Splits "/slug rest" into a known command and the rest. The slug must be followed by a
 * space, so "/ne" while typing is still text. Unknown slugs stay part of the question.
 */
export function parseCommand(text: string, skills: Skill[]): Parsed {
  const m = text.match(/^\/([a-z0-9-]+)\s/);
  if (!m) return { command: null, rest: text };
  const slug = m[1];
  const builtin = BUILTINS.find((b) => b.slug === slug);
  const skill = builtin ? undefined : skills.find((s) => s.slug === slug);
  const command: Command | null = builtin ? { kind: "builtin", builtin } : skill ? { kind: "skill", skill } : null;
  return command ? { command, rest: text.slice(m[0].length) } : { command: null, rest: text };
}

/** The question text for a command and the rest; the inverse of `parseCommand`. */
export const withCommand = (slug: string, rest: string) => `/${slug} ${rest}`;

export type MenuItem = { kind: "builtin" | "skill"; slug: string; description: string } | { kind: "create" };

/** The text after "/" while the menu should show, else null: "/" at the start and no space yet. */
export function menuQuery(text: string): string | null {
  const m = text.match(/^\/([a-z0-9-]*)$/i);
  return m ? m[1].toLowerCase() : null;
}

/**
 * Commands matching what was typed after "/": names starting with it first, then names or
 * descriptions containing it. "Create skill…" is always last.
 */
export function menuItems(query: string, skills: Skill[]): MenuItem[] {
  const all = [
    ...BUILTINS.map((b) => ({ kind: "builtin" as const, slug: b.slug, description: b.description })),
    ...skills.map((s) => ({ kind: "skill" as const, slug: s.slug, description: s.description || s.instruction })),
  ];
  const q = query.toLowerCase();
  const prefix = all.filter((i) => i.slug.startsWith(q));
  const contains = all.filter((i) => !prefix.includes(i) && (i.slug.includes(q) || i.description.toLowerCase().includes(q)));
  return [...prefix, ...contains, { kind: "create" }];
}

/** What the question asks for while typing (`draft`), or on ↵ (`action`). */
export type Plan =
  | {
      kind: "draft";
      question: string;
      task: Extract<GenerationTask, { kind: "draft" }>;
      skill: string | null;
      /** Instant local answers (no AI) may stand in. Not with a skill, whose instruction may change the answer. */
      instant: boolean;
    }
  | { kind: "action"; command: ActionCommand; question: string; skill: null };

export interface PlanContext {
  /** The query last run in the tab. */
  previous: PreviousQuery | null;
  /** The SQL in the editor now. */
  sql: string;
}

/** Turns the question text into the request to send. */
export function planRequest(text: string, skills: Skill[], ctx: PlanContext): Plan {
  const { command, rest } = parseCommand(text, skills);
  if (isAction(command)) return { kind: "action", command: command.builtin.slug, question: rest, skill: null };
  const draft = (task: Extract<GenerationTask, { kind: "draft" }>, skill: string | null = null) => ({
    kind: "draft" as const,
    question: rest,
    task,
    skill,
    // A skill's instruction can change the answer, and /followup must not be guessed locally.
    instant: !skill && !task.forceFollowUp,
  });
  if (!command) return draft({ kind: "draft", previous: ctx.previous });
  if (command.kind === "skill") return draft({ kind: "draft", previous: ctx.previous }, command.skill.slug);
  if (command.builtin.slug === "new") return draft({ kind: "draft", previous: null });
  // /followup: the last run query, else whatever is in the editor.
  const previous = ctx.previous ?? (ctx.sql.trim() ? { question: "", sql: ctx.sql } : null);
  return draft({ kind: "draft", previous, forceFollowUp: true });
}

/** The task for an action command, run on the editor's SQL and the last database error. */
export function actionTask(command: ActionCommand, sql: string, error: string | null): GenerationTask {
  switch (command) {
    case "explain":
      return { kind: "explain", sql };
    case "improve":
      return { kind: "improve", sql };
    case "fix":
      return { kind: "fix", sql, error: error ?? "" };
  }
}

export const SKILL_LIMITS = { slug: 32, description: 100, instruction: 1000 };

/** What's wrong with a skill before saving, or null. Same rules as `validate` in src-tauri/src/commands/skills.rs. */
export function skillError(skill: Skill, existing: Skill[], original: string | null): string | null {
  const slug = skill.slug.trim().replace(/^\//, "");
  if (!slug) return "Give the skill a name.";
  if (slug.length > SKILL_LIMITS.slug) return `Keep the name to ${SKILL_LIMITS.slug} characters.`;
  if (!/^[a-z0-9-]+$/.test(slug)) return "Use only lowercase letters, numbers and dashes in the name.";
  if (BUILTINS.some((b) => b.slug === slug)) return `/${slug} is a built-in command. Pick another name.`;
  if (existing.some((s) => s.slug === slug && s.slug !== original)) return `You already have a skill called /${slug}.`;
  if ([...skill.description.trim()].length > SKILL_LIMITS.description)
    return `Keep the description to ${SKILL_LIMITS.description} characters.`;
  const instruction = skill.instruction.trim();
  if (!instruction) return "Write the instruction.";
  if ([...instruction].length > SKILL_LIMITS.instruction) return `Keep the instruction to ${SKILL_LIMITS.instruction} characters.`;
  return null;
}

/** Suggestions shown while there are no skills; not installed until the person saves one. */
export const EXAMPLE_SKILLS: Skill[] = [
  { slug: "pii-safe", description: "Leave out personal data", instruction: "Never select email, phone or address columns." },
  { slug: "this-month", description: "Only this month", instruction: "Only include rows from the current calendar month." },
  { slug: "small", description: "At most 20 rows", instruction: "Return at most 20 rows." },
];
