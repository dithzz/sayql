import type { SettingsSection } from "../settings/SettingsPage";

/** SayQL's docs site; pages are built from docs-site/pages (their `slug`). */
export const DOCS_URL = "https://sayql.dev/docs/";

/** The docs page for each Settings section; every slug exists in docs-site/pages. */
export const SECTION_DOCS: Record<SettingsSection, string> = {
  ai: "ai-providers/",
  agent: "agent-mode/",
  skills: "asking/#skills",
  voice: "asking/#say-it",
  mcp: "mcp/",
  activity: "privacy/#your-activity",
  team: "teams/",
  about: "",
};

export const docsUrl = (section: SettingsSection) => DOCS_URL + SECTION_DOCS[section];
