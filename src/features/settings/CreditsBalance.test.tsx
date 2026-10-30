// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CreditsBalance, LOW_CREDIT_USD, lowCreditText } from "./CreditsBalance";

const creditsBalance = vi.fn<() => Promise<number | null>>();
vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: { creditsBalance: () => creditsBalance() },
}));

afterEach(() => {
  cleanup();
  creditsBalance.mockReset();
});

describe("SayQL AI credit balance", () => {
  it("warns when it's low or gone, and says nothing when there's plenty", () => {
    expect(lowCreditText(42)).toBeNull();
    expect(lowCreditText(LOW_CREDIT_USD - 0.01)).toContain("running low: $4.99 left");
    expect(lowCreditText(0)).toContain("has run out");
    for (const usd of [0, 1]) expect(lowCreditText(usd)).toContain("hello@azenvoc.com");
  });

  it("shows what's left, with what it costs", async () => {
    creditsBalance.mockResolvedValue(12.5);
    render(<CreditsBalance />);
    expect(await screen.findByText(/Balance: \$12\.50 left/)).toBeTruthy();
    expect(screen.getByText(/plus 20%/)).toBeTruthy();
  });

  it("shows the low-credit warning in Settings too", async () => {
    creditsBalance.mockResolvedValue(2);
    render(<CreditsBalance />);
    expect(await screen.findByText(/running low: \$2\.00 left/)).toBeTruthy();
  });

  it("says when the balance can't be checked", async () => {
    creditsBalance.mockRejectedValue("Could not reach SayQL AI credits: offline");
    render(<CreditsBalance />);
    expect(await screen.findByText(/Couldn't check the balance: Could not reach/)).toBeTruthy();
  });
});
