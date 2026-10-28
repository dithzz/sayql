// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AuditAgent, AuditEvent } from "../../api";
import { AuditButton, AuditDialog, auditButtonLabel } from "./AuditRun";
import { SUGGESTIONS_CHANGED } from "./useNoteSuggestions";

const claude = (patch: Partial<AuditAgent> = {}): AuditAgent => ({
  id: "claude",
  label: "Claude Code",
  path: "/Users/me/.local/bin/claude",
  version: "2.1.267 (Claude Code)",
  supported: true,
  ready: true,
  problem: null,
  installUrl: "https://code.claude.com/docs/en/setup",
  ...patch,
});
const codex: AuditAgent = {
  id: "codex",
  label: "Codex CLI",
  path: "/opt/homebrew/bin/codex",
  version: null,
  supported: false,
  ready: false,
  problem: "Codex reads code by running shell commands.",
  installUrl: "https://developers.openai.com/codex/cli",
};

let agents: AuditAgent[] = [];
let folder: string | null = null;
let emit: (e: AuditEvent) => void = () => {};
const startAudit = vi.fn((_run: string, _agent: string, _id: string, _db: string | null, _folder: string) => Promise.resolve());
const cancelAudit = vi.fn((_run: string) => Promise.resolve());
const openAuditInTerminal = vi.fn((_agent: string, _id: string, _db: string | null, _folder: string) => Promise.resolve());
const openUrl = vi.fn();

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: (u: string) => openUrl(u) }));
vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: {
    auditAgents: () => Promise.resolve(agents),
    auditFolder: () => Promise.resolve(folder),
    chooseAuditFolder: () => Promise.resolve("/Users/me/code/shop"),
    startAudit: (r: string, a: string, i: string, d: string | null, f: string) => startAudit(r, a, i, d, f),
    cancelAudit: (r: string) => cancelAudit(r),
    openAuditInTerminal: (a: string, i: string, d: string | null, f: string) => openAuditInTerminal(a, i, d, f),
    auditPrompt: () => Promise.resolve("Audit the database"),
  },
  subscribeToAudit: (h: (e: AuditEvent) => void) => {
    emit = h;
    return Promise.resolve(() => {});
  },
}));

const target = { connectionId: "c1", name: "Shop", database: "sales" };

beforeEach(() => {
  agents = [claude(), codex];
  folder = null;
  startAudit.mockClear();
  cancelAudit.mockClear();
  openAuditInTerminal.mockClear();
});
afterEach(cleanup);

it("names the agent that can run, else asks", () => {
  expect(auditButtonLabel(null)).toBe("Audit with…");
  expect(auditButtonLabel([claude(), codex])).toBe("Audit with Claude Code");
  expect(auditButtonLabel([claude({ ready: false, problem: "too old" }), codex])).toBe("Audit with…");
  expect(auditButtonLabel([claude(), { ...claude(), id: "other", label: "Other" }])).toBe("Audit with…");
});

it("opens the run dialog from the button, or hands it off", async () => {
  const onOpen = vi.fn();
  render(<AuditButton target={target} onOpen={onOpen} />);
  const button = await screen.findByRole("button", { name: "Audit with Claude Code on Shop" });
  fireEvent.click(button);
  expect(onOpen).toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).toBeNull();
  cleanup();
  render(<AuditButton target={target} />);
  fireEvent.click(await screen.findByRole("button", { name: "Audit with Claude Code on Shop" }));
  expect(await screen.findByRole("dialog", { name: "Audit with Claude Code" })).toBeTruthy();
});

it("needs a folder, then streams progress and ends with the suggestions to review", async () => {
  const onReview = vi.fn();
  const onClose = vi.fn();
  const changed = vi.fn();
  window.addEventListener(SUGGESTIONS_CHANGED, changed);
  render(<AuditDialog target={target} onReview={onReview} onClose={onClose} />);
  const start = await screen.findByRole("button", { name: "Start audit" });
  expect((start as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(/No commands, no file changes, no web/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  expect(await screen.findByText("/Users/me/code/shop")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Open in Terminal" }));
  expect(openAuditInTerminal).toHaveBeenCalledWith("claude", "c1", "sales", "/Users/me/code/shop");

  fireEvent.click(screen.getByRole("button", { name: "Start audit" }));
  await waitFor(() => expect(startAudit).toHaveBeenCalled());
  const [runId, agent, id, db, dir] = startAudit.mock.calls[0];
  expect([agent, id, db, dir]).toEqual(["claude", "c1", "sales", "/Users/me/code/shop"]);
  act(() => {
    emit({ type: "line", runId: "someone-else", kind: "step", text: "Not this run" });
    emit({ type: "line", runId, kind: "step", text: "Reading src/db/orders.ts" });
    emit({ type: "line", runId, kind: "note", text: "Suggested a note: orders.total is in cents" });
  });
  expect(screen.getByText("Reading src/db/orders.ts")).toBeTruthy();
  expect(screen.queryByText("Not this run")).toBeNull();
  expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  act(() => emit({ type: "done", runId, outcome: "ok", suggested: 6, summary: "I suggested 6 notes.", error: null }));
  expect(screen.getByText("6 notes suggested.")).toBeTruthy();
  expect(screen.getByText("I suggested 6 notes.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Review suggestions" }));
  expect(onClose).toHaveBeenCalled();
  expect(onReview).toHaveBeenCalled();
  expect(changed).toHaveBeenCalled();
  window.removeEventListener(SUGGESTIONS_CHANGED, changed);
});

it("cancels a run, and shows why one couldn't start", async () => {
  folder = "/Users/me/code/shop";
  render(<AuditDialog target={target} onClose={() => {}} />);
  fireEvent.click(await screen.findByRole("button", { name: "Start audit" }));
  await waitFor(() => expect(startAudit).toHaveBeenCalled());
  const runId = startAudit.mock.calls[0][0];
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(cancelAudit).toHaveBeenCalledWith(runId);
  act(() => emit({ type: "done", runId, outcome: "error", suggested: 0, summary: null, error: "Claude Code isn't signed in: run `claude` once in a terminal and sign in, then try again." }));
  expect(screen.getByRole("alert").textContent).toContain("isn't signed in");

  startAudit.mockImplementationOnce(() => Promise.reject("Claude Code is blocked from using SayQL."));
  fireEvent.click(screen.getByRole("button", { name: "Run again" }));
  fireEvent.click(screen.getByRole("button", { name: "Start audit" }));
  expect(await screen.findByText("Claude Code is blocked from using SayQL.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Start audit" })).toBeTruthy();
});

it("without an agent it can run, explains, links to the install page and keeps the prompt", async () => {
  agents = [claude({ path: null, version: null, ready: false }), codex];
  render(<AuditDialog target={target} onClose={() => {}} />);
  expect(await screen.findByText(/Codex reads code by running shell commands/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Start audit" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Install Claude Code" }));
  expect(openUrl).toHaveBeenCalledWith("https://code.claude.com/docs/en/setup");
  expect(screen.getByRole("button", { name: "Copy audit prompt" })).toBeTruthy();
});

it("asks for the connection when it has none", async () => {
  folder = "/Users/me/code/shop";
  const connections = [
    { id: "c1", name: "Shop" },
    { id: "c2", name: "Analytics" },
    { id: "team:t1", name: "Team DB" },
  ] as never;
  render(<AuditDialog connections={connections} onClose={() => {}} />);
  const select = (await screen.findByRole("combobox", { name: "Connection to audit" })) as HTMLSelectElement;
  expect([...select.options].map((o) => o.text)).toEqual(["Shop", "Analytics"]);
  fireEvent.change(select, { target: { value: "c2" } });
  await screen.findByText("/Users/me/code/shop");
  fireEvent.click(screen.getByRole("button", { name: "Start audit" }));
  await waitFor(() => expect(startAudit).toHaveBeenCalled());
  expect(startAudit.mock.calls[0].slice(1)).toEqual(["claude", "c2", null, "/Users/me/code/shop"]);
});
