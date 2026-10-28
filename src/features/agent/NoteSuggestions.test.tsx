// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { DbNote, NoteSuggestion } from "../../api";
import { SuggestedNotes } from "../settings/SuggestedNotes";
import { NoteSuggestions } from "./NoteSuggestions";
import { SUGGESTIONS_CHANGED, suggestionFor } from "./useNoteSuggestions";

let waiting: NoteSuggestion[] = [];
const saved: DbNote[] = [];
vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: {
    listNoteSuggestions: () => Promise.resolve(waiting),
    acceptNoteSuggestion: (id: string) => {
      const s = waiting.find((x) => x.id === id)!;
      waiting = waiting.filter((x) => x.id !== id);
      const note = { id: "n1", connectionId: s.connectionId, database: s.database, text: s.text, updatedAt: 1 };
      saved.push(note);
      return Promise.resolve(note);
    },
    dismissNoteSuggestion: (id: string) => {
      waiting = waiting.filter((x) => x.id !== id);
      return Promise.resolve();
    },
  },
}));

afterEach(cleanup);

const suggestion = (id: string, text: string, app = "Claude Code"): NoteSuggestion => ({
  id,
  connectionId: "c1",
  database: "shop",
  text,
  evidence: "no references in api/ since commit a1b2c3",
  app,
  createdAt: 1,
});

it("shows the app, note and evidence, and accepts or dismisses", async () => {
  waiting = [suggestion("s1", "orders_legacy is unused; use orders_v2"), suggestion("s2", "status 3 = cancelled", "Cursor")];
  const changed = vi.fn();
  window.addEventListener(SUGGESTIONS_CHANGED, changed);
  render(<SuggestedNotes connections={[{ id: "c1", name: "Shop" } as never]} />);

  expect(await screen.findByText("orders_legacy is unused; use orders_v2")).toBeTruthy();
  expect(screen.getAllByText(/no references in api\//)).toHaveLength(2);
  expect(screen.getByText("Cursor")).toBeTruthy();
  expect(screen.getAllByText(/Shop · shop/)).toHaveLength(2);

  fireEvent.click(screen.getAllByRole("button", { name: "Accept" })[0]);
  await waitFor(() => expect(screen.queryByText("orders_legacy is unused; use orders_v2")).toBeNull());
  expect(saved.map((n) => n.text)).toEqual(["orders_legacy is unused; use orders_v2"]);
  expect(changed).toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  // The section goes away once nothing is waiting.
  await waitFor(() => expect(screen.queryByText("Suggested notes")).toBeNull());
  expect(saved).toHaveLength(1);
  window.removeEventListener(SUGGESTIONS_CHANGED, changed);
});

it("shows why an accept failed", async () => {
  const onAccept = vi.fn(() => Promise.reject("A note describes the database, not the data in it."));
  render(<NoteSuggestions suggestions={[suggestion("s1", "x")]} onAccept={onAccept} onDismiss={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Accept" }));
  expect(await screen.findByText(/describes the database/)).toBeTruthy();
});

it("matches suggestions to the open database", () => {
  const s = suggestion("s1", "x");
  expect(suggestionFor(s, { connectionId: "c1", database: "shop" })).toBe(true);
  expect(suggestionFor(s, { connectionId: "c1", database: null })).toBe(false);
  expect(suggestionFor({ ...s, database: null }, { connectionId: "c1", database: null })).toBe(true);
  expect(suggestionFor(s, null)).toBe(false);
});
