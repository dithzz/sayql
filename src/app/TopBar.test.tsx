// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { TopBar } from "./TopBar";

// The theme switch beside it needs a ThemeProvider; it isn't what these tests are about.
vi.mock("../features/theme", () => ({ ThemeToggle: () => null }));

afterEach(cleanup);

const props = { active: null, databaseName: null, schema: null, onNewConnection: vi.fn() };

it("counts suggested notes waiting on the settings button", () => {
  const onOpenSettings = vi.fn();
  render(<TopBar {...props} onOpenSettings={onOpenSettings} pendingSuggestions={2} />);
  const gear = screen.getByRole("button", { name: "Settings: 2 suggested notes waiting" });
  expect(gear.parentElement?.textContent).toBe("2");
  fireEvent.click(gear);
  expect(onOpenSettings).toHaveBeenCalled();
});

it("shows no count when nothing is waiting", () => {
  render(<TopBar {...props} onOpenSettings={vi.fn()} />);
  const gear = screen.getByRole("button", { name: "Settings" });
  expect(gear.parentElement?.textContent).toBe("");
});
