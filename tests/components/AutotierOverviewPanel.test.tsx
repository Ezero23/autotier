import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AutotierOverviewPanel } from "@/components/autotier/AutotierOverviewPanel";

const queryMocks = vi.hoisted(() => ({
  config: vi.fn(),
  decisions: vi.fn(),
}));

vi.mock("@/lib/query/autotier", () => ({
  useAutotierRoutingConfig: () => queryMocks.config(),
  useAutotierDecisions: () => queryMocks.decisions(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => {
      if (key === "autotier.console.status.candidateCoverage") {
        return `coverage ${values?.percent}`;
      }
      return key;
    },
  }),
}));

function decision(overrides: Record<string, unknown> = {}) {
  return {
    recommended_slot: "mid",
    candidate_model: "model-mid",
    candidate_provider: "provider-a",
    is_complete: true,
    user_label: null,
    ...overrides,
  };
}

describe("AutotierOverviewPanel", () => {
  beforeEach(() => {
    queryMocks.config.mockReturnValue({ data: { mode: "shadow" } });
    queryMocks.decisions.mockReturnValue({
      data: {
        total: 3,
        items: [
          decision({ recommended_slot: "cheap" }),
          decision({ recommended_slot: "strong" }),
          decision({
            candidate_model: null,
            candidate_provider: null,
            is_complete: false,
          }),
        ],
      },
    });
  });

  it("leads a stopped installation to connection setup and summarizes coverage", () => {
    const openSetup = vi.fn();

    render(
      <AutotierOverviewPanel
        isProxyRunning={false}
        takeoverStatus={undefined}
        onOpenDecisions={vi.fn()}
        onOpenSetup={openSetup}
      />,
    );

    expect(screen.getByText("coverage 67%")).toBeInTheDocument();
    expect(
      screen.getByText("autotier.console.next.startProxyTitle"),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getAllByRole("button", {
        name: /autotier.console.next.openSetup/,
      })[0],
    );
    expect(openSetup).toHaveBeenCalledTimes(1);
  });

  it("sends a healthy unlabeled dataset to Shadow review", () => {
    const openDecisions = vi.fn();
    queryMocks.decisions.mockReturnValue({
      data: {
        total: 2,
        items: [decision(), decision({ recommended_slot: "strong" })],
      },
    });

    render(
      <AutotierOverviewPanel
        isProxyRunning
        takeoverStatus={{
          claude: true,
          codex: false,
          gemini: false,
          grokbuild: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        }}
        onOpenDecisions={openDecisions}
        onOpenSetup={vi.fn()}
      />,
    );

    expect(
      screen.getByText("autotier.console.next.reviewTitle"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getAllByRole("button", {
        name: /autotier.console.next.reviewAction/,
      })[0],
    );
    expect(openDecisions).toHaveBeenCalledTimes(1);
  });
});
