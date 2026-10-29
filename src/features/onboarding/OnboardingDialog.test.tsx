// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AiProvider, Settings } from "../../api";
import { OnboardingDialog, type OnboardingDialogProps } from "./OnboardingDialog";

vi.mock("../../api/commands", () => ({ commands: { listModels: vi.fn(), saveSettings: vi.fn() } }));

const provider = (id: string, label: string, extra: Partial<AiProvider> = {}): AiProvider =>
  ({ id, label, note: `${label} note`, api: "openai", key: "required", keyPlaceholder: "", local: false, defaultBaseUrl: "x", baseUrl: "",
     typingModel: "", expertModel: "", agentModel: "", hasKey: false, ready: false, ...extra }) as AiProvider;

const settings = (aiReady: boolean) =>
  ({
    provider: "anthropic",
    aiReady,
    workspaceId: "",
    providers: [
      provider("anthropic", "Anthropic Claude", { api: "anthropic" }),
      provider("sayql", "SayQL AI credits", { api: "anthropic", note: "Claude without an Anthropic account." }),
      provider("openai", "OpenAI"),
      provider("gemini", "Google Gemini"),
      provider("ollama", "Ollama", { key: "none", local: true }),
      provider("openrouter", "OpenRouter"),
    ],
  }) as unknown as Settings;

function renderStep(step: OnboardingDialogProps["step"], over: Partial<OnboardingDialogProps> = {}) {
  const p: OnboardingDialogProps = {
    settings: settings(true), step, onStep: vi.fn(), onSettingsSaved: vi.fn(), demoAvailable: true, onUseDemo: vi.fn(), onConnectOwn: vi.fn(),
    examples: ["Top 5 customers"], onTry: vi.fn(), onOpenSettings: vi.fn(), onOpenAiApps: vi.fn(), onTeach: vi.fn(), onFinish: vi.fn(), ...over,
  };
  render(<OnboardingDialog {...p} />);
  return p;
}

describe("first-run setup", () => {
  afterEach(cleanup);

  it("offers SayQL AI credits as a provider, with where to get a key", () => {
    renderStep("ai");
    const card = screen.getByRole("radio", { name: /SayQL AI credits/ });
    expect(card.textContent).toContain("No Anthropic account");
    fireEvent.click(card);
    expect(screen.getByText("Credit key")).toBeTruthy();
    expect(screen.getByText(/hello@azenvoc\.com/)).toBeTruthy();
  });

  it("ends with a short, optional way into AI agents and governance", () => {
    const p = renderStep("try");
    expect(screen.getByText("Let AI agents in safely")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Connect an AI app" }));
    expect(p.onOpenAiApps).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Teach SayQL" }));
    expect(p.onTeach).toHaveBeenCalled();
    // Skippable: the usual way out is still there.
    expect(screen.getByRole("button", { name: "I'll type my own" })).toBeTruthy();
  });

  it("shows the panel with AI off too, and no Teach button without a database", () => {
    renderStep("try", { settings: settings(false), onTeach: undefined });
    expect(screen.getByRole("button", { name: "Connect an AI app" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Teach SayQL" })).toBeNull();
  });
});
