// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../api/commands";
import { ConnectionDialog, newConnection } from "./ConnectionDialog";

vi.mock("../../api/commands", () => ({ commands: { deleteConnection: vi.fn() } }));
const deleteConnection = vi.mocked(commands.deleteConnection);

const existing = { ...newConnection(), id: "c1", name: "Shop", database: "shop" };

function renderDialog() {
  const onDeleted = vi.fn();
  render(<ConnectionDialog initial={existing} onClose={() => {}} onSaved={() => {}} onDeleted={onDeleted} />);
  return onDeleted;
}

describe("ConnectionDialog delete", () => {
  afterEach(() => {
    cleanup();
    deleteConnection.mockReset();
  });

  it("deletes after a second click", async () => {
    deleteConnection.mockResolvedValue(undefined);
    const onDeleted = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Click again to delete/ }));
    await vi.waitFor(() => expect(onDeleted).toHaveBeenCalledWith("c1"));
  });

  it("shows the error and keeps the dialog open when the delete fails", async () => {
    deleteConnection.mockRejectedValue("keychain is locked");
    const onDeleted = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Click again to delete/ }));
    expect((await screen.findByRole("alert")).textContent).toContain("Could not delete the connection: keychain is locked");
    expect(onDeleted).not.toHaveBeenCalled();
    // Back to the first step: another click asks again rather than deleting straight away.
    expect(screen.getByRole("button", { name: /^Delete$/ })).toBeTruthy();
  });
});
