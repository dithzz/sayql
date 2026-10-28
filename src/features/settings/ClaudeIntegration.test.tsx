// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AgentAccess, McpApp, NoteSuggestion } from "../../api";
import type { NoteSuggestionsApi } from "../agent";
import { ClaudeIntegration } from "./ClaudeIntegration";

let apps: McpApp[] = [];
const setMcpAppBlocked = vi.fn((name: string, blocked: boolean) => {
  apps = apps.map((a) => (a.name === name ? { ...a, blocked } : a));
  return Promise.resolve(apps);
});
const access: AgentAccess = { enabled: false, results: "off", apps: "counts", askBeforeSend: true, neverSend: [] };
vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: {
    mcpSetup: () => Promise.resolve({ path: "/Applications/SayQL.app/Contents/MacOS/sayql-mcp", exists: true, appimage: false }),
    mcpApps: () => Promise.resolve(apps),
    setMcpAppBlocked: (n: string, b: boolean) => setMcpAppBlocked(n, b),
    mcpActivity: () => Promise.resolve([]),
    agentAccess: () => Promise.resolve(access),
  },
}));

const suggestion: NoteSuggestion = { id: "s1", connectionId: "c1", database: null, text: "status 3 = cancelled", evidence: "enum in api/", app: "Cursor", createdAt: 1 };
const api = (suggestions: NoteSuggestion[]): NoteSuggestionsApi => ({
  suggestions,
  accept: vi.fn(),
  dismiss: vi.fn(),
  reload: vi.fn(),
});
const connections = [{ id: "c1", name: "Shop", kind: "sqlite" } as never];
const provider = { label: "Anthropic", local: false };

beforeEach(() => {
  apps = [];
  setMcpAppBlocked.mockClear();
});
afterEach(cleanup);

it("opens on Access, switches tabs, and comes back to the last one", async () => {
  const { unmount } = render(<ClaudeIntegration connections={connections} provider={provider} suggestions={api([])} />);
  const access = screen.getByRole("tab", { name: "Access" });
  expect(access.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe(access.id);
  expect(await screen.findByText(/No AI app has connected yet/)).toBeTruthy();

  fireEvent.click(screen.getByRole("tab", { name: "Activity" }));
  expect(screen.getByRole("tab", { name: "Activity" }).getAttribute("aria-selected")).toBe("true");
  expect(await screen.findByText(/Nothing yet\. Queries from Claude/)).toBeTruthy();

  // Arrow keys move along the tab list.
  fireEvent.keyDown(screen.getByRole("tab", { name: "Activity" }), { key: "ArrowRight" });
  expect(screen.getByRole("tab", { name: "Suggestions" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByText("No suggested notes are waiting.")).toBeTruthy();
  unmount();

  // Settings opened again in this session: back on Suggestions.
  render(<ClaudeIntegration connections={connections} provider={provider} suggestions={api([])} />);
  expect(screen.getByRole("tab", { name: "Suggestions" }).getAttribute("aria-selected")).toBe("true");
  fireEvent.click(screen.getByRole("tab", { name: "Access" }));
});

it("opens on the tab asked for, with the waiting count on Suggestions", () => {
  render(<ClaudeIntegration connections={connections} provider={provider} suggestions={api([suggestion])} tab="suggestions" />);
  const tab = screen.getByRole("tab", { name: /Suggestions/ });
  expect(tab.getAttribute("aria-selected")).toBe("true");
  expect(tab.textContent).toBe("Suggestions1");
  expect(screen.getByText("status 3 = cancelled")).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "Access" }));
});

it("shows the setup open when no app has connected, closed once one has", async () => {
  const { unmount } = render(<ClaudeIntegration connections={connections} provider={provider} suggestions={api([])} />);
  const toggle = await screen.findByRole("button", { name: "Connect an app" });
  await waitFor(() => expect(toggle.getAttribute("aria-expanded")).toBe("true"));
  expect(screen.getByText(/claude mcp add --scope user sayql/)).toBeTruthy();
  unmount();

  apps = [{ name: "Cursor", blocked: false, queries: 4, lastUsed: 1 }];
  render(<ClaudeIntegration connections={connections} provider={provider} suggestions={api([])} />);
  expect(await screen.findByText(/4 requests in the log/)).toBeTruthy();
  const closed = await screen.findByRole("button", { name: "Connect an app" });
  expect(closed.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText(/claude mcp add/)).toBeNull();
  fireEvent.click(closed);
  expect(screen.getByText(/claude mcp add --scope user sayql/)).toBeTruthy();
});

it("blocks and allows an app", async () => {
  apps = [{ name: "Cursor", blocked: false, queries: 1, lastUsed: 2 }];
  render(<ClaudeIntegration connections={connections} provider={provider} suggestions={api([])} />);
  expect(await screen.findByText("Allowed")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Block Cursor" }));
  await waitFor(() => expect(setMcpAppBlocked).toHaveBeenCalledWith("Cursor", true));
  expect(await screen.findByRole("button", { name: "Allow Cursor" })).toBeTruthy();
  expect(screen.getByText("Blocked")).toBeTruthy();
});
