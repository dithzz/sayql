import { describe, expect, it } from "vitest";
import { AGENT_OPTIONS, APP_OPTIONS, LEVELS, NEVER_SEND_SCOPE, agentLevelShort, appLevelShort, withDefault } from "./resultLevels";
import { describeAccess } from "./useAgentAccess";

describe("one vocabulary for what the AI gets back", () => {
  it("names the same level the same way in agent mode and AI apps", () => {
    const agent = Object.fromEntries(AGENT_OPTIONS.map((o) => [o.level, o.label]));
    const apps = Object.fromEntries(APP_OPTIONS.map((o) => [o.level, o.label]));
    expect(agent.nothing).toBe(apps.nothing);
    expect(agent.counts).toBe(apps.counts);
    // Agent mode's rows are masked rows too, capped at 20 per query.
    expect(agent.masked).toBe(`${apps.masked} (up to 20 per query)`);
    expect(apps.all).toBe("All rows");
    expect(agent.all).toBeUndefined();
  });

  it("never calls a level Off, so 'Turn on agent mode' doesn't offer 'Off' first", () => {
    for (const o of [...AGENT_OPTIONS, ...APP_OPTIONS]) expect(o.label).not.toMatch(/^off\b/i);
    expect(AGENT_OPTIONS[0].label).toBe("Nothing (names and row counts only)");
  });

  it("keeps the stored values and their order", () => {
    expect(AGENT_OPTIONS.map((o) => o.value)).toEqual(["off", "counts", "rows"]);
    expect(APP_OPTIONS.map((o) => o.value)).toEqual(["off", "counts", "masked", "all"]);
  });

  it("marks the backend's defaults: nothing for agent mode, counts for AI apps", () => {
    expect(AGENT_OPTIONS.filter((o) => o.isDefault).map((o) => o.value)).toEqual(["off"]);
    expect(APP_OPTIONS.filter((o) => o.isDefault).map((o) => o.value)).toEqual(["counts"]);
    expect(withDefault(APP_OPTIONS[1])).toBe("Counts and summaries · default");
    expect(withDefault(APP_OPTIONS[2])).toBe("Rows, masked");
  });

  it("only claims masking where the backend masks", () => {
    expect(LEVELS.all.detail).not.toMatch(/mask/i);
    expect(AGENT_OPTIONS[2].detail).toMatch(/replaced by tokens/);
    expect(LEVELS.nothing.detail).toMatch(/No values/);
  });

  it("uses the same words in status lines", () => {
    expect(agentLevelShort("off")).toBe("nothing (names and row counts only)");
    expect(agentLevelShort("rows")).toBe("rows, masked (up to 20 per query)");
    expect(appLevelShort("counts")).toBe("counts and summaries");
    expect(describeAccess({ enabled: true, results: "counts", apps: "counts", askBeforeSend: true, neverSend: [] })).toBe(
      "Results to AI: counts and summaries · Ask before send",
    );
  });

  it("says never-send covers Investigate and AI apps", () => {
    expect(NEVER_SEND_SCOPE).toMatch(/Investigate/);
    expect(NEVER_SEND_SCOPE).toMatch(/AI apps over MCP/);
  });
});
