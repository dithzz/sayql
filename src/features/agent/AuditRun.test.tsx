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
let savedModel = "opus";
const setAuditModel = vi.fn((m: string) => {
  savedModel = m;
  return Promise.resolve();
});

let payment: { payment: string; hasCreditKey: boolean } = { payment: "subscription", hasCreditKey: false };
const setAuditPayment = vi.fn((_p: string) => Promise.resolve());

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
    auditModel: () => Promise.resolve(savedModel),
    setAuditModel: (m: string) => setAuditModel(m),
    auditPayment: () => Promise.resolve(payment),
    setAuditPayment: (p: string) => setAuditPayment(p),
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
  payment = { payment: "subscription", hasCreditKey: false };
  setAuditPayment.mockClear();
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
  act(() =>
    emit({
      type: "done",
      runId,
      outcome: "ok",
      suggested: 6,
      summary: "## Audit\n\nI suggested **6 notes**.\n\n- `orders.status` codes\n- see [docs](https://example.com)\n\n<img src=x onerror=alert(1)>",
      error: null,
    }),
  );
  expect(screen.getByText("6 notes suggested.")).toBeTruthy();
  // The summary is Markdown, rendered as elements: never as HTML.
  expect(screen.getByRole("heading", { name: "Audit" })).toBeTruthy();
  expect(screen.getByText("6 notes").tagName).toBe("STRONG");
  expect(screen.getByText("orders.status").tagName).toBe("CODE");
  fireEvent.click(screen.getByRole("link", { name: "docs" }));
  expect(openUrl).toHaveBeenCalledWith("https://example.com");
  expect(document.querySelector("img")).toBeNull();
  expect(screen.getByText("<img src=x onerror=alert(1)>")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Review suggestions" }));
  expect(onClose).toHaveBeenCalled();
  expect(onReview).toHaveBeenCalled();
  expect(changed).toHaveBeenCalled();
  window.removeEventListener(SUGGESTIONS_CHANGED, changed);
});

it("runs on the saved model, Sonnet by default, and remembers a new choice", async () => {
  folder = "/Users/me/code/shop";
  savedModel = "opus";
  setAuditModel.mockClear();
  render(<AuditDialog target={target} onClose={() => {}} />);
  const select = (await screen.findByRole("combobox", { name: "Model for the audit" })) as HTMLSelectElement;
  await waitFor(() => expect(select.value).toBe("opus"));
  expect([...select.options].map((o) => o.text)).toEqual(["Sonnet (cheaper)", "Opus", "My Claude Code default"]);
  expect(screen.getByText(/counts against your own Claude plan/)).toBeTruthy();
  fireEvent.change(select, { target: { value: "sonnet" } });
  await waitFor(() => expect(setAuditModel).toHaveBeenCalledWith("sonnet"));
  expect(select.value).toBe("sonnet");
  expect(screen.getByText(/Sonnet is cheaper/)).toBeTruthy();
});

it("pays with SayQL AI credits when a credit key is saved, and remembers it", async () => {
  folder = "/Users/me/code/shop";
  savedModel = "default";
  payment = { payment: "subscription", hasCreditKey: true };
  render(<AuditDialog target={target} onClose={() => {}} />);
  const pay = (await screen.findByRole("combobox", { name: "Pay for the audit with" })) as HTMLSelectElement;
  expect([...pay.options].map((o) => [o.text, o.disabled])).toEqual([
    ["My Claude Code sign-in", false],
    ["SayQL AI credits", false],
  ]);
  expect(pay.value).toBe("subscription");
  const terminal = screen.getByRole("button", { name: "Open in Terminal" }) as HTMLButtonElement;
  expect(terminal.disabled).toBe(false);

  fireEvent.change(pay, { target: { value: "credits" } });
  await waitFor(() => expect(setAuditPayment).toHaveBeenCalledWith("credits"));
  expect(pay.value).toBe("credits");
  expect(screen.getByText(/go through SayQL's credits service to Anthropic and are charged to your credit balance/)).toBeTruthy();
  expect(screen.getByText(/about \$0\.40 for a typical audit on Sonnet/)).toBeTruthy();
  expect(screen.getByText(/SayQL keeps only token counts, never your code or the answers/)).toBeTruthy();
  expect(screen.queryByText(/counts against your own Claude plan/)).toBeNull();
  // Credits cover Sonnet and Opus: "my default" runs on Sonnet and isn't offered.
  const model = screen.getByRole("combobox", { name: "Model for the audit" }) as HTMLSelectElement;
  expect([...model.options].map((o) => o.text)).toEqual(["Sonnet (cheaper)", "Opus"]);
  expect(model.value).toBe("sonnet");
  fireEvent.change(model, { target: { value: "opus" } });
  expect(await screen.findByText(/about \$0\.80 for a typical audit on Opus/)).toBeTruthy();
  // The key stays in the app's own run: no terminal.
  expect(terminal.disabled).toBe(true);
  expect(terminal.title).toContain("works for runs inside SayQL");
  fireEvent.click(screen.getByRole("button", { name: "Start audit" }));
  await waitFor(() => expect(startAudit).toHaveBeenCalled());
});

it("offers credits only with a credit key, and links to where it's added", async () => {
  folder = "/Users/me/code/shop";
  // Credits were chosen, then the key was removed: the run uses the person's own sign-in.
  payment = { payment: "subscription", hasCreditKey: false };
  const onOpenAiSettings = vi.fn();
  render(<AuditDialog target={target} onClose={() => {}} onOpenAiSettings={onOpenAiSettings} />);
  const pay = (await screen.findByRole("combobox", { name: "Pay for the audit with" })) as HTMLSelectElement;
  expect(pay.value).toBe("subscription");
  expect([...pay.options].find((o) => o.value === "credits")?.disabled).toBe(true);
  fireEvent.change(pay, { target: { value: "credits" } });
  expect(setAuditPayment).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Add a credit key in Settings → AI provider" }));
  expect(onOpenAiSettings).toHaveBeenCalled();
  cleanup();
  // Without a way to open Settings, the same words, as text.
  render(<AuditDialog target={target} onClose={() => {}} />);
  expect(await screen.findByText(/Add a credit key in Settings → AI provider to pay with SayQL AI credits/)).toBeTruthy();
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
