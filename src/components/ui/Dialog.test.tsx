// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Dialog } from "./Dialog";

describe("Dialog", () => {
  afterEach(cleanup);

  it("closes on Escape without the page behind seeing it", () => {
    const onClose = vi.fn();
    const behind = vi.fn();
    window.addEventListener("keydown", behind);
    render(
      <Dialog title="Pick columns" onClose={onClose}>
        <input aria-label="Filter" />
      </Dialog>,
    );
    fireEvent.keyDown(screen.getByLabelText("Filter"), { key: "Escape" });
    window.removeEventListener("keydown", behind);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(behind).not.toHaveBeenCalled();
  });

  it("keeps Tab inside the dialog", () => {
    render(
      <Dialog title="Pick columns" onClose={() => {}} actions={<button>Save</button>}>
        <input aria-label="Filter" />
      </Dialog>,
    );
    const save = screen.getByRole("button", { name: "Save" });
    save.focus();
    fireEvent.keyDown(save, { key: "Tab" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" }));
  });
});
