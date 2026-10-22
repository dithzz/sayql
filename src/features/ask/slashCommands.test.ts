import { describe, expect, it } from "vitest";
import type { Skill } from "../../api";
import { actionTask, commandSlug, menuItems, menuQuery, parseCommand, planRequest, skillError, withCommand } from "./slashCommands";

const pii: Skill = { slug: "pii-safe", description: "Leave out personal data", instruction: "Never select email, phone or address columns." };
const month: Skill = { slug: "this-month", description: "", instruction: "Only rows from this month." };
const skills = [pii, month];
const previous = { question: "list orders", sql: "SELECT * FROM orders;" };

const slugs = (q: string) => menuItems(q, skills).map((i) => (i.kind === "create" ? "+create" : i.slug));

describe("menuQuery", () => {
  it("opens on a slash at the start, until a space is typed", () => {
    expect(menuQuery("/")).toBe("");
    expect(menuQuery("/Ex")).toBe("ex");
    expect(menuQuery("/explain ")).toBeNull();
    expect(menuQuery("show /x")).toBeNull();
    expect(menuQuery("")).toBeNull();
  });
});

describe("menuItems", () => {
  it("lists every command, then Create skill", () => {
    expect(slugs("")).toEqual(["followup", "new", "explain", "fix", "improve", "pii-safe", "this-month", "+create"]);
  });

  it("puts name matches first, then names or descriptions containing the text", () => {
    expect(slugs("p")).toEqual(["pii-safe", "followup", "explain", "improve", "+create"]);
    expect(slugs("month")).toEqual(["this-month", "+create"]);
    expect(slugs("personal")).toEqual(["pii-safe", "+create"]);
    expect(slugs("zzz")).toEqual(["+create"]);
  });

  it("describes a skill with no description by its instruction", () => {
    expect(menuItems("this", skills)[0]).toMatchObject({ slug: "this-month", description: "Only rows from this month." });
  });
});

describe("parseCommand", () => {
  it("splits a known command from the rest", () => {
    const p = parseCommand("/pii-safe list customers", skills);
    expect(p.command && commandSlug(p.command)).toBe("pii-safe");
    expect(p.rest).toBe("list customers");
    expect(parseCommand("/explain ", skills)).toMatchObject({ command: { kind: "builtin" }, rest: "" });
  });

  it("round-trips with withCommand, keeping spaces the person typed", () => {
    const text = withCommand("new", " show  customers");
    expect(parseCommand(text, skills).rest).toBe(" show  customers");
  });

  it("leaves unknown or unfinished commands as text", () => {
    expect(parseCommand("/nope show customers", skills)).toEqual({ command: null, rest: "/nope show customers" });
    expect(parseCommand("/new", skills)).toEqual({ command: null, rest: "/new" });
    expect(parseCommand("show /new customers", skills).command).toBeNull();
  });
});

describe("planRequest", () => {
  const ctx = { previous, sql: "SELECT 1;" };

  it("drafts a plain question on top of the last query", () => {
    expect(planRequest("only shipped", skills, ctx)).toEqual({
      kind: "draft",
      question: "only shipped",
      task: { kind: "draft", previous },
      skill: null,
      instant: true,
    });
  });

  it("/new ignores the last query", () => {
    expect(planRequest("/new list customers", skills, ctx)).toMatchObject({ question: "list customers", task: { previous: null }, instant: true });
  });

  it("/followup forces an edit, of the editor's SQL when nothing was run", () => {
    expect(planRequest("/followup count them", skills, ctx)).toMatchObject({
      task: { previous, forceFollowUp: true },
      instant: false,
    });
    expect(planRequest("/followup count them", skills, { previous: null, sql: "SELECT 1;" })).toMatchObject({
      task: { previous: { question: "", sql: "SELECT 1;" }, forceFollowUp: true },
    });
  });

  it("a skill goes with the request and skips instant answers", () => {
    expect(planRequest("/pii-safe list customers", skills, ctx)).toMatchObject({
      kind: "draft",
      question: "list customers",
      task: { previous },
      skill: "pii-safe",
      instant: false,
    });
  });

  it("explain, fix and improve wait for ↵", () => {
    expect(planRequest("/explain why the join", skills, ctx)).toEqual({ kind: "action", command: "explain", question: "why the join", skill: null });
    expect(planRequest("/fix ", skills, ctx)).toMatchObject({ kind: "action", command: "fix", question: "" });
  });
});

describe("actionTask", () => {
  it("reuses the fix and improve tasks", () => {
    expect(actionTask("fix", "SELECT x", "no such column: x")).toEqual({ kind: "fix", sql: "SELECT x", error: "no such column: x" });
    expect(actionTask("fix", "SELECT x", null)).toEqual({ kind: "fix", sql: "SELECT x", error: "" });
    expect(actionTask("improve", "SELECT x", "ignored")).toEqual({ kind: "improve", sql: "SELECT x" });
    expect(actionTask("explain", "SELECT x", null)).toEqual({ kind: "explain", sql: "SELECT x" });
  });
});

describe("skillError", () => {
  const skill = (slug: string, instruction = "Hide emails.") => ({ slug, description: "", instruction });

  it("accepts a valid skill, with or without a leading slash", () => {
    expect(skillError(skill("pii-2"), skills, null)).toBeNull();
    expect(skillError(skill("/pii-2"), skills, null)).toBeNull();
  });

  it("rejects bad, long, built-in and duplicate names", () => {
    expect(skillError(skill(""), [], null)).toMatch(/name/);
    expect(skillError(skill("PII safe"), [], null)).toMatch(/lowercase/);
    expect(skillError(skill("a".repeat(33)), [], null)).toMatch(/32/);
    expect(skillError(skill("fix"), [], null)).toMatch(/built-in/);
    expect(skillError(skill("pii-safe"), skills, null)).toMatch(/already/);
    expect(skillError(skill("pii-safe"), skills, "pii-safe")).toBeNull();
  });

  it("needs an instruction of at most 1000 characters", () => {
    expect(skillError(skill("x", "  "), [], null)).toMatch(/instruction/);
    expect(skillError(skill("x", "a".repeat(1001)), [], null)).toMatch(/1000/);
    expect(skillError({ ...skill("x"), description: "d".repeat(101) }, [], null)).toMatch(/100/);
  });
});
