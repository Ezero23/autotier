import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { AutotierProviderPricingPanel } from "@/components/autotier/AutotierProviderPricingPanel";
import type {
  AutotierProviderModelPricing,
  AutotierProviderModelPricingInput,
} from "@/lib/api/autotier";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const TAURI = "http://tauri.local";

function installPricingApi(initial: AutotierProviderModelPricing[] = []) {
  const store = [...initial];
  server.use(
    http.post(`${TAURI}/autotier_list_provider_model_pricing`, () =>
      HttpResponse.json(store),
    ),
    http.post(
      `${TAURI}/autotier_upsert_provider_model_pricing`,
      async ({ request }) => {
        const body = (await request.json()) as {
          input?: AutotierProviderModelPricingInput;
        };
        const input = body.input;
        if (!input?.model_id?.trim()) {
          return HttpResponse.text("model_id is required", { status: 500 });
        }
        const saved: AutotierProviderModelPricing = {
          ...input,
          provider_id: "p1",
          model_id: input.model_id.trim(),
          display_name: input.display_name?.trim() || input.model_id.trim(),
          price_source: input.price_source?.trim() || "manual",
          observed_at: Date.now(),
        };
        const index = store.findIndex((row) => row.model_id === saved.model_id);
        if (index >= 0) store[index] = saved;
        else store.push(saved);
        return HttpResponse.json(saved);
      },
    ),
    http.post(
      `${TAURI}/autotier_delete_provider_model_pricing`,
      async ({ request }) => {
        const body = (await request.json()) as {
          providerId?: string;
          modelId?: string;
        };
        const index = store.findIndex(
          (row) =>
            row.provider_id === body.providerId &&
            row.model_id === body.modelId,
        );
        if (index >= 0) store.splice(index, 1);
        return HttpResponse.json(index >= 0 ? 1 : 0);
      },
    ),
  );
  return store;
}

function renderPanel(providerId = "p1") {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <AutotierProviderPricingPanel providerId={providerId} />
    </QueryClientProvider>,
  );
}

describe("AutotierProviderPricingPanel", () => {
  it("saves and deletes a provider-specific pricing snapshot", async () => {
    const user = userEvent.setup();
    const store = installPricingApi();
    renderPanel();

    expect(
      await screen.findByText(
        /No Provider-specific pricing snapshots saved yet/i,
      ),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: /Add pricing snapshot/i }),
    );

    const newRow = screen.getByTestId("autotier-pricing-row-new");
    const fields = within(newRow).getAllByRole("textbox");
    await user.type(fields[0], "gpt-5-mini");
    await user.type(fields[1], "GPT-5 Mini");
    await user.clear(fields[2]);
    await user.type(fields[2], "0.25");
    await user.click(within(newRow).getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(
        screen.getByTestId("autotier-pricing-row-gpt-5-mini"),
      ).toBeInTheDocument(),
    );
    expect(store).toHaveLength(1);
    expect(store[0]).toMatchObject({
      provider_id: "p1",
      model_id: "gpt-5-mini",
      display_name: "GPT-5 Mini",
      input_cost_per_million: "0.25",
    });

    const savedRow = screen.getByTestId("autotier-pricing-row-gpt-5-mini");
    await user.click(
      within(savedRow).getByRole("button", {
        name: /Delete pricing snapshot/i,
      }),
    );
    await waitFor(() => expect(store).toHaveLength(0));
  });
});
