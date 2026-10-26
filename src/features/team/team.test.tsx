// @vitest-environment jsdom
import { cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConnectionConfig, TeamAccount, TeamStatus } from "../../api";
import { DatabaseNavigator } from "../navigator/DatabaseNavigator";
import { TeamSection } from "./TeamSection";
import { TEAMS_URL, licenceText } from "./teamText";
import { useTeamConnections } from "./useTeamConnections";

// Every command a team feature could send; a signed-out app must call none of them.
const teamStatus = vi.fn<() => Promise<TeamStatus | null>>();
const teamConnections = vi.fn<() => Promise<ConnectionConfig[]>>();
const teamSignOut = vi.fn<() => Promise<void>>();
const teamSignIn = vi.fn<(server: string, code: string) => Promise<TeamAccount>>();
vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: {
    teamStatus: () => teamStatus(),
    teamConnections: () => teamConnections(),
    teamSignOut: () => teamSignOut(),
    teamSignIn: (s: string, c: string) => teamSignIn(s, c),
  },
}));
const openUrl = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: (u: string) => openUrl(u) }));

const account: TeamAccount = { server: "https://sayql.acme.test", email: "ana@acme.test", name: "Ana", role: "member" };
const info = { name: "SayQL server", version: "0.1.0", licensed: true, plan: "team", licensee: "Acme", expires: "2027-04-30", users: 3, userLimit: 10 };

const conn = (id: string, name: string, team?: ConnectionConfig["team"]): ConnectionConfig => ({
  id, name, kind: "postgres", host: "", port: null, database: "", user: "", file: "", ssl: false, trustCert: false, team,
});

describe("SayQL Team", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(cleanup);

  it("signed out, sends nothing anywhere: no status, no connections", async () => {
    const apply = vi.fn();
    renderHook(() => useTeamConnections(null, apply, vi.fn()));
    render(<TeamSection account={null} onChange={vi.fn()} />);
    expect(screen.getByText("Using SayQL with a team? Sign in to your company's SayQL server.")).toBeTruthy();
    await new Promise((r) => setTimeout(r, 10));
    expect(apply).toHaveBeenCalledWith([]);
    for (const command of [teamStatus, teamConnections, teamSignOut, teamSignIn]) expect(command).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("SayQL for teams"));
    expect(openUrl).toHaveBeenCalledWith(TEAMS_URL);
    // Opening the dialog doesn't sign in: the person confirms first.
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(screen.getByRole("dialog", { name: "Sign in to your team" })).toBeTruthy();
    expect(teamSignIn).not.toHaveBeenCalled();
  });

  it("signed in, loads the team's connections and shows the account and licence", async () => {
    teamConnections.mockResolvedValue([conn("team:c_1", "Production", { database: "app", notes: "" })]);
    teamStatus.mockResolvedValue({ account, server: info });
    teamSignOut.mockResolvedValue();
    const apply = vi.fn();
    renderHook(() => useTeamConnections(account, apply, vi.fn()));
    await waitFor(() => expect(apply).toHaveBeenCalledWith([expect.objectContaining({ id: "team:c_1" })]));

    const onChange = vi.fn();
    render(<TeamSection account={account} onChange={onChange} />);
    await waitFor(() => expect(screen.getByText("Team plan for Acme · 3 of 10 seats · until 2027-04-30")).toBeTruthy());
    expect(screen.getByText("ana@acme.test")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(null));
  });

  it("says when the server has no licence", () => {
    expect(licenceText({ ...info, licensed: false })).toContain("No valid licence");
  });

  it("lists team connections in their own group, with no edit or delete", () => {
    render(
      <DatabaseNavigator
        connections={[conn("c1", "Local"), conn("team:c_1", "Production", { database: "app", notes: "Read replica" })]}
        activeId={null} databases={{}} databaseName={null} schema={null} loading={false}
        onOpen={vi.fn()} onOpenDatabase={vi.fn()} onAdd={vi.fn()} onEdit={vi.fn()} onDeleteConnection={vi.fn()} onRefresh={vi.fn()}
        onPreviewTable={vi.fn()} onNewTab={vi.fn()} onOpenSql={vi.fn()} selectedTables={[]} onSelectTables={vi.fn()}
        scripts={[]} onOpenScript={vi.fn()} onDeleteScript={vi.fn()} notes={[]}
        onEditNote={vi.fn()} onDeleteNote={vi.fn()} onAgentSettings={vi.fn()}
      />,
    );
    expect(screen.getByText("Team")).toBeTruthy();
    expect(screen.getByText("Production")).toBeTruthy();
    // One Edit button: the local connection's.
    expect(screen.getAllByRole("button", { name: "Edit connection" })).toHaveLength(1);
    fireEvent.contextMenu(screen.getByText("Production"));
    expect(screen.queryByText("Edit connection…")).toBeNull();
    expect(screen.queryByText("Delete connection…")).toBeNull();
    expect(screen.getByText("Agent settings…")).toBeTruthy();
  });
});
