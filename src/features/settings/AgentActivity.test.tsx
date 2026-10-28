// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { McpActivity, McpApp } from "../../api";
import { AgentActivity } from "./AgentActivity";

const log: McpActivity[] = [
  { at: 2, app: "Cursor", tool: "run_query", connectionId: "c1", connection: "Shop", sql: "SELECT email FROM customers", status: "refused", detail: "never-send column email" },
  { at: 1, app: "Claude Code", tool: "run_query", connectionId: "c1", connection: "Shop", sql: "SELECT COUNT(*) FROM orders", status: "ok", rows: 1 },
];
let apps: McpApp[] = [
  { name: "Cursor", blocked: false, queries: 1, lastUsed: 2 },
  { name: "Claude Code", blocked: false, queries: 1, lastUsed: 1 },
];
const setMcpAppBlocked = vi.fn((name: string, blocked: boolean) => {
  apps = apps.map((a) => (a.name === name ? { ...a, blocked } : a));
  return Promise.resolve(apps);
});
vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: {
    mcpApps: () => Promise.resolve(apps),
    mcpActivity: () => Promise.resolve(log),
    setMcpAppBlocked: (n: string, b: boolean) => setMcpAppBlocked(n, b),
    clearMcpActivity: () => Promise.resolve(),
  },
}));

afterEach(cleanup);

it("lists what AI apps ran, refusals included, and blocks an app", async () => {
  render(<AgentActivity />);
  expect(await screen.findByText("SELECT email FROM customers")).toBeTruthy();
  expect(screen.getByText(/never-send column email/)).toBeTruthy();
  expect(screen.getByText(/· 1 rows/)).toBeTruthy();

  fireEvent.click(screen.getAllByRole("button", { name: "Block" })[0]);
  await waitFor(() => expect(setMcpAppBlocked).toHaveBeenCalledWith("Cursor", true));
  expect(await screen.findByRole("button", { name: "Allow" })).toBeTruthy();
  expect(screen.getByText(/Blocked/)).toBeTruthy();
});
