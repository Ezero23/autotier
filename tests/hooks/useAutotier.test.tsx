import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import {
  type AutotierProviderSlot,
  type AutotierRoutingConfig,
} from "@/lib/api/autotier";
import {
  autotierKeys,
  useAutotierProviderSlots,
  useAutotierRequiredSlots,
  useAutotierRoutingConfig,
  useSaveAutotierRoutingConfig,
} from "@/lib/query/autotier";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const TAURI = "http://tauri.local";

const SAMPLE_CONFIG: AutotierRoutingConfig = {
  mode: "shadow",
  advisory_candidate: null,
  retention_days: 30,
  raw_prompt_opt_in: false,
  classifier_version: "rules-v0.2",
  feature_version: "claude-extractor-v0.2",
  policy_version: "shadow-policy-v0.2",
  capability_table_version: "capability-table-v0.1",
  cost_model_version: "cost-model-v0.1",
  cache_stats_version: "cache-stats-v0.1",
  updated_at: 1,
  degraded_from: null,
};

const SAMPLE_SLOT: AutotierProviderSlot = {
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
};

function wrapper() {
  const queryClient = createTestQueryClient();
  const Provider = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { wrapper: Provider, queryClient };
}

describe("autotier query/mutation (MSW)", () => {
  it("loads routing config including version fields", async () => {
    server.use(
      http.post(`${TAURI}/autotier_get_routing_config`, () =>
        HttpResponse.json({
          ...SAMPLE_CONFIG,
          api_key: "sk-ant-should-not-leak",
        }),
      ),
    );
    const { wrapper: W } = wrapper();
    const { result } = renderHook(() => useAutotierRoutingConfig(), {
      wrapper: W,
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.mode).toBe("shadow");
    expect(result.current.data?.capability_table_version).toBe(
      "capability-table-v0.1",
    );
    expect(result.current.data?.cost_model_version).toBe("cost-model-v0.1");
    expect(result.current.data?.cache_stats_version).toBe("cache-stats-v0.1");
    expect(JSON.stringify(result.current.data).toLowerCase()).not.toContain(
      "api_key",
    );
    expect(JSON.stringify(result.current.data)).not.toContain("sk-ant");
  });

  it("maps illegal mode save errors to the error enum", async () => {
    server.use(
      http.post(`${TAURI}/autotier_save_routing_config`, () =>
        HttpResponse.text("illegal routing mode: banana", { status: 500 }),
      ),
    );
    const { wrapper: W } = wrapper();
    const { result } = renderHook(() => useSaveAutotierRoutingConfig(), {
      wrapper: W,
    });
    await act(async () => {
      await expect(
        result.current.mutateAsync({
          mode: "off",
          advisory_candidate: null,
          retention_days: 30,
        }),
      ).rejects.toMatchObject({
        name: "AutotierApiError",
        code: "illegal_mode",
      });
    });
  });

  it("loads slots and required status without copying decision logic", async () => {
    server.use(
      http.post(
        `${TAURI}/autotier_list_provider_slots`,
        async ({ request }) => {
          const body = (await request.json()) as { providerId?: string };
          expect(body.providerId).toBe("p1");
          return HttpResponse.json([
            { ...SAMPLE_SLOT, authorization: "Bearer leaked" },
          ]);
        },
      ),
      http.post(`${TAURI}/autotier_required_slots_status`, () =>
        HttpResponse.json({
          provider_id: "p1",
          complete: false,
          present: ["cheap"],
          missing: ["mid", "strong"],
        }),
      ),
    );
    const { wrapper: W, queryClient } = wrapper();
    const slots = renderHook(() => useAutotierProviderSlots("p1"), {
      wrapper: W,
    });
    const required = renderHook(() => useAutotierRequiredSlots("p1"), {
      wrapper: W,
    });
    await waitFor(() => expect(slots.result.current.isSuccess).toBe(true));
    await waitFor(() => expect(required.result.current.isSuccess).toBe(true));
    expect(slots.result.current.data?.[0]?.slot).toBe("cheap");
    expect(JSON.stringify(slots.result.current.data)).not.toContain("Bearer");
    expect(required.result.current.data?.missing).toEqual(["mid", "strong"]);
    expect(queryClient.getQueryData(autotierKeys.slots("p1"))).toBeTruthy();
  });
});
