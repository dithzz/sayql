// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { AgentAccess, ConnectionConfig } from "../../api";
import { AppAccessList } from "./AppAccessList";

const saved: AgentAccess = { enabled: true, results: "rows", apps: "counts", askBeforeSend: false, neverSend: ["notes"] };
const agentAccess = vi.fn((_id: string) => Promise.resolve(saved));
const saveAgentAccess = vi.fn((_id: string, a: AgentAccess) => Promise.resolve(a));
vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: {
    agentAccess: (id: string) => agentAccess(id),
    saveAgentAccess: (id: string, a: AgentAccess) => saveAgentAccess(id, a),
  },
}));

const conn = (id: string, name: string): ConnectionConfig => ({
  id, name, kind: "postgres", host: "", port: null, database: "", user: "", file: "", ssl: false, trustCert: false,
});

afterEach(cleanup);

it("shows counts by default and saves a new choice without touching agent mode", async () => {
  render(<AppAccessList connections={[conn("c1", "Production"), conn("team:x", "Team DB")]} />);
  const select = await screen.findByLabelText("What AI apps may see on Production");
  await waitFor(() => expect((select as HTMLSelectElement).value).toBe("counts"));
  // Team connections aren't served over MCP, so they aren't listed.
  expect(screen.queryByLabelText("What AI apps may see on Team DB")).toBeNull();
  expect(agentAccess).toHaveBeenCalledTimes(1);

  fireEvent.change(select, { target: { value: "all" } });
  await waitFor(() => expect(saveAgentAccess).toHaveBeenCalled());
  expect(saveAgentAccess).toHaveBeenCalledWith("c1", { ...saved, apps: "all" });
});
