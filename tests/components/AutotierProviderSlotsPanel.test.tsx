import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { AutotierProviderSlotsPanel } from "@/components/autotier/AutotierProviderSlotsPanel";
import {
  duplicateModelGroups,
  invalidSlotReasons,
  isLiveEligibleCapability,
  rowsForSlotUi,
  shouldShowLiveReady,
} from "@/components/autotier/autotierSlotUi";
import type { AutotierProviderSlot } from "@/lib/api/autotier";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const TAURI = "http://tauri.local";

function sampleSlot(
  overrides: Partial<AutotierProviderSlot> = {},
): AutotierProviderSlot {
  return {
    provider_id: "p1",
    slot: "cheap",
    model_id: "claude-haiku",
    capability_status: "unknown",
    supports_tools: true,
    supports_streaming: true,
    supports_vision: false,
    context_limit: 200000,
    api_format: "anthropic",
    pricing_source: "builtin",
    capability_source: "manual",
    verified_at: null,
    created_at: 1,
    updated_at: 1,
    ...overrides,
  };
}

function installSlotApi(initial: AutotierProviderSlot[] = []) {
  const store = { slots: [...initial] };
  const required = () => {
    const names = ["cheap", "mid", "strong"] as const;
    const present = names.filter((slot) =>
      store.slots.some((row) => row.slot === slot),
    );
    return {
      provider_id: "p1",
      complete: present.length === names.length,
      present,
      missing: names.filter((slot) => !present.includes(slot)),
    };
  };
  server.use(
    http.post(`${TAURI}/autotier_list_provider_slots`, () =>
      HttpResponse.json(store.slots),
    ),
    http.post(`${TAURI}/autotier_required_slots_status`, () =>
      HttpResponse.json(required()),
    ),
    http.post(`${TAURI}/autotier_upsert_provider_slot`, async ({ request }) => {
      const body = (await request.json()) as { slot?: AutotierProviderSlot };
      const incoming = body.slot;
      if (!incoming?.model_id?.trim()) {
        return HttpResponse.text("model_id is required", { status: 500 });
      }
      const saved: AutotierProviderSlot = {
        ...incoming,
        created_at: incoming.created_at || 1,
        updated_at: Date.now(),
      };
      const index = store.slots.findIndex((row) => row.slot === saved.slot);
      if (index >= 0) store.slots[index] = saved;
      else store.slots.push(saved);
      return HttpResponse.json(saved);
    }),
  );
  return store;
}

function renderPanel(knownModelIds?: readonly string[], providerId = "p1") {
  const queryClient = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <AutotierProviderSlotsPanel
        providerId={providerId}
        knownModelIds={knownModelIds}
      />
    </QueryClientProvider>,
  );
  return { ...view, queryClient };
}

describe("autotierSlotUi helpers", () => {
  it("keeps required drafts when the list is empty", () => {
    const rows = rowsForSlotUi("p1", []);
    expect(rows.map((row) => row.slot)).toEqual(["cheap", "mid", "strong"]);
    expect(rows.every((row) => row.model_id === "")).toBe(true);
  });

  it("flags empty, unknown, stale, failed, and missing models as invalid", () => {
    expect(
      invalidSlotReasons({
        slot: "cheap",
        model_id: "",
        capability_status: "unknown",
      }),
    ).toEqual(["empty_model"]);
    expect(
      invalidSlotReasons({
        slot: "ultra",
        model_id: "m",
        capability_status: "failed",
      }),
    ).toEqual(["unknown_slot", "failed_capability"]);
    expect(
      invalidSlotReasons({
        slot: "mid",
        model_id: "gone",
        capability_status: "stale",
        knownModelIds: ["kept"],
      }),
    ).toEqual(["stale_capability", "model_missing"]);
  });

  it("groups duplicate models without swapping them", () => {
    const groups = duplicateModelGroups([
      { slot: "cheap", model_id: "same" },
      { slot: "mid", model_id: "same" },
      { slot: "strong", model_id: "other" },
    ]);
    expect(groups.get("same")).toEqual(["cheap", "mid"]);
    expect(groups.has("other")).toBe(false);
  });

  it("never shows Live-ready in v0.1, even for verified capability", () => {
    expect(isLiveEligibleCapability("verified")).toBe(true);
    expect(isLiveEligibleCapability("probed")).toBe(true);
    expect(isLiveEligibleCapability("unknown")).toBe(false);
    expect(shouldShowLiveReady("verified")).toBe(false);
    expect(shouldShowLiveReady("probed")).toBe(false);
  });
});

describe("AutotierProviderSlotsPanel", () => {
  it("covers loading, empty drafts, and error retry", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    server.use(
      http.post(`${TAURI}/autotier_list_provider_slots`, async () => {
        await gate;
        return HttpResponse.json([]);
      }),
      http.post(`${TAURI}/autotier_required_slots_status`, async () => {
        await gate;
        return HttpResponse.json({
          provider_id: "p1",
          complete: false,
          present: [],
          missing: ["cheap", "mid", "strong"],
        });
      }),
    );
    const { unmount } = renderPanel();
    expect(screen.getByRole("status")).toHaveTextContent(/loading slots/i);
    release?.();
    expect(
      await screen.findByTestId("autotier-slots-empty"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Cheap model")).toBeInTheDocument();
    expect(screen.getByLabelText("Mid model")).toBeInTheDocument();
    expect(screen.getByLabelText("Strong model")).toBeInTheDocument();
    unmount();

    server.use(
      http.post(`${TAURI}/autotier_list_provider_slots`, () =>
        HttpResponse.text("db locked", { status: 500 }),
      ),
      http.post(`${TAURI}/autotier_required_slots_status`, () =>
        HttpResponse.text("db locked", { status: 500 }),
      ),
    );
    const errorClient = createTestQueryClient();
    render(
      <QueryClientProvider client={errorClient}>
        <AutotierProviderSlotsPanel providerId="p1" />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByTestId("autotier-slots-error"),
    ).toBeInTheDocument();
    installSlotApi([]);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(screen.getByTestId("autotier-slots-empty")).toBeInTheDocument(),
    );
  });

  it("lets a new user fill Cheap/Mid/Strong and warns on the same model", async () => {
    const user = userEvent.setup();
    installSlotApi([]);
    renderPanel();
    await screen.findByTestId("autotier-slots-empty");

    await user.type(screen.getByLabelText("Cheap model"), "claude-haiku");
    await user.type(screen.getByLabelText("Mid model"), "claude-haiku");
    await user.type(screen.getByLabelText("Strong model"), "claude-opus");

    expect(screen.getByTestId("autotier-duplicate-model")).toHaveTextContent(
      "claude-haiku",
    );
    expect(screen.getByTestId("autotier-duplicate-model")).toHaveTextContent(
      "not actually distinct",
    );

    await user.click(screen.getByRole("button", { name: "Save Cheap slot" }));
    await user.click(screen.getByRole("button", { name: "Save Mid slot" }));
    await user.click(screen.getByRole("button", { name: "Save Strong slot" }));

    await waitFor(() =>
      expect(screen.getByTestId("autotier-slots-complete")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("autotier-shadow-explainer")).toHaveTextContent(
      "does not change the outbound model or provider",
    );
    const cheapRow = screen.getByTestId("autotier-slot-row-cheap");
    expect(within(cheapRow).getAllByText("unknown").length).toBeGreaterThan(0);
    expect(within(cheapRow).getByText(/Pricing source/)).toBeInTheDocument();
    expect(within(cheapRow).getByText(/Capability source/)).toBeInTheDocument();
  });

  it("marks a disappeared model invalid and never shows Live-ready", async () => {
    installSlotApi([
      sampleSlot({
        slot: "cheap",
        model_id: "gone-model",
        capability_status: "verified",
        pricing_source: "models.dev",
        capability_source: "probe",
      }),
      sampleSlot({
        slot: "mid",
        model_id: "kept",
        capability_status: "probed",
      }),
      sampleSlot({
        slot: "strong",
        model_id: "kept-strong",
        capability_status: "verified",
      }),
    ]);
    renderPanel(["kept", "kept-strong"]);
    const cheapRow = await screen.findByTestId("autotier-slot-row-cheap");
    expect(cheapRow).toHaveTextContent(
      "This model is no longer on the provider",
    );
    expect(screen.queryByTestId("autotier-live-ready")).not.toBeInTheDocument();
    expect(screen.queryByText(/live-ready/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("autotier-slots-complete")).toBeInTheDocument();
  });

  it("follows keyboard focus Cheap → Mid → Strong when saves are disabled", async () => {
    const user = userEvent.setup();
    installSlotApi([]);
    renderPanel();
    const cheap = await screen.findByLabelText("Cheap model");
    const mid = screen.getByLabelText("Mid model");
    const strong = screen.getByLabelText("Strong model");
    cheap.focus();
    expect(cheap).toHaveFocus();
    await user.tab();
    expect(mid).toHaveFocus();
    await user.tab();
    expect(strong).toHaveFocus();
  });
});
