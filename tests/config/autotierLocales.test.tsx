import type { ReactNode } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { describe, expect, it, beforeAll } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";
import {
  AUTOTIER_I18N_KEYS,
  autotierTranslations,
} from "@/i18n/autotier/translations";
import {
  AutotierRoutingSettingsPanel,
  buildRoutingSaveMode,
  forcedChoiceFromMode,
  shouldShowLiveRoutingUi,
} from "@/components/autotier/AutotierRoutingSettingsPanel";
import type { AutotierRoutingConfig } from "@/lib/api/autotier";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const TAURI = "http://tauri.local";

type TranslationTree = Record<string, unknown>;

function flattenStrings(
  value: unknown,
  path: string[] = [],
  result = new Map<string, string>(),
): Map<string, string> {
  if (typeof value === "string") {
    result.set(path.join("."), value);
  } else if (typeof value === "object" && value !== null) {
    for (const [key, child] of Object.entries(value)) {
      flattenStrings(child, [...path, key], result);
    }
  }
  return result;
}

function getTranslation(locale: TranslationTree, path: string): unknown {
  return path.split(".").reduce<unknown>((value, key) => {
    if (!value || typeof value !== "object") return undefined;
    return (value as Record<string, unknown>)[key];
  }, locale);
}

function interpolationVariables(value: string): string[] {
  return Array.from(value.matchAll(/\{\{\s*([^}]+?)\s*\}\}/g), ([, name]) =>
    name.trim(),
  ).sort();
}

const referenceAutotier = flattenStrings(autotierTranslations.en);

const locales = [
  ["en", { ...en, autotier: autotierTranslations.en }],
  ["ja", { ...ja, autotier: autotierTranslations.ja }],
  ["zh", { ...zh, autotier: autotierTranslations.zh }],
  ["zh-TW", { ...zhTW, autotier: autotierTranslations["zh-TW"] }],
] as const;

const SAMPLE_CONFIG: AutotierRoutingConfig = {
  mode: "shadow",
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

function installRoutingApi(initial: AutotierRoutingConfig = SAMPLE_CONFIG) {
  let config = { ...initial };
  server.use(
    http.post(`${TAURI}/autotier_get_routing_config`, () =>
      HttpResponse.json(config),
    ),
    http.post(`${TAURI}/autotier_save_routing_config`, async ({ request }) => {
      const body = (await request.json()) as {
        input?: { mode?: string; retention_days?: number };
      };
      config = {
        ...config,
        mode: "shadow",
        retention_days: body.input?.retention_days ?? config.retention_days,
        degraded_from:
          (body.input?.mode?.startsWith("forced_") ?? false)
            ? (body.input?.mode ?? null)
            : null,
        updated_at: Date.now(),
      };
      return HttpResponse.json(config);
    }),
    http.post(
      `${TAURI}/autotier_clear_decisions`,
      () => new HttpResponse(null, { status: 200 }),
    ),
    http.post(`${TAURI}/autotier_prune_decisions`, () => HttpResponse.json(3)),
  );
}

function renderRoutingPanel() {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return render(<AutotierRoutingSettingsPanel />, { wrapper });
}

beforeAll(async () => {
  await i18n.use(initReactI18next).init({
    lng: "en",
    fallbackLng: "en",
    resources: {
      en: { translation: { ...en, autotier: autotierTranslations.en } },
    },
    interpolation: { escapeValue: false },
  });
});

describe("autotier locale contract", () => {
  it.each(locales)("defines every AutoTier key in %s", (_name, tree) => {
    const translations = flattenStrings(tree as TranslationTree);
    const missing = AUTOTIER_I18N_KEYS.filter((key) => {
      const value = translations.get(key);
      return typeof value !== "string" || value.trim().length === 0;
    });
    expect(missing).toEqual([]);
  });

  it.each(locales.slice(1))(
    "preserves interpolation variables against English in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      const mismatched = [...referenceAutotier.entries()].flatMap(
        ([relativePath, expected]) => {
          const key = `autotier.${relativePath}`;
          const actual = translations.get(key);
          if (actual === undefined) return [key];
          return interpolationVariables(actual).join("\0") !==
            interpolationVariables(expected).join("\0")
            ? [key]
            : [];
        },
      );
      expect(mismatched).toEqual([]);
    },
  );

  it("lists slot and routing namespaces in the reference tree", () => {
    expect(getTranslation(autotierTranslations.en, "slots.title")).toBeTruthy();
    expect(
      getTranslation(autotierTranslations.en, "routing.title"),
    ).toBeTruthy();
    expect(AUTOTIER_I18N_KEYS.length).toBeGreaterThan(40);
  });
});

describe("autotier routing helpers", () => {
  it("maps forced modes and never exposes Live UI in v0.1", () => {
    expect(forcedChoiceFromMode("forced_mid")).toBe("mid");
    expect(buildRoutingSaveMode("shadow", "strong")).toBe("forced_strong");
    expect(buildRoutingSaveMode("off", "cheap")).toBe("off");
    expect(shouldShowLiveRoutingUi()).toBe(false);
  });
});

describe("AutotierRoutingSettingsPanel", () => {
  it("shows Off/Shadow, retention, privacy, versions, hint, and no Live UI", async () => {
    installRoutingApi();
    renderRoutingPanel();
    expect(
      await screen.findByTestId("autotier-routing-settings-panel"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("autotier-no-live-banner")).toBeInTheDocument();
    expect(screen.queryByText(/canary live/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("autotier-privacy-copy")).toBeInTheDocument();
    expect(screen.getByTestId("autotier-version-stamps")).toBeInTheDocument();
    expect(screen.getByText(/Not connected/i)).toBeInTheDocument();
    expect(screen.getByTestId("autotier-canary-gate")).toHaveTextContent(
      /Not met/i,
    );
  });

  it("shows forced candidate advisory and does not execute Live routing", async () => {
    const user = userEvent.setup();
    installRoutingApi();
    renderRoutingPanel();
    await screen.findByLabelText(/Forced candidate slot/i);
    await user.click(screen.getByLabelText(/Forced candidate slot/i));
    await user.click(
      screen.getByRole("option", { name: /Force Mid candidate/i }),
    );
    expect(screen.getByTestId("autotier-forced-advisory")).toHaveTextContent(
      /Advisory only — not executed/i,
    );
    expect(screen.getByTestId("autotier-forced-advisory")).toHaveTextContent(
      /Actual outbound models stay on the baseline path/i,
    );
    await user.click(
      screen.getByRole("button", { name: /Save routing settings/i }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("autotier-degraded-warning")).toHaveTextContent(
        /forced_mid/i,
      ),
    );
  });

  it("exposes labeled controls for keyboard focus order", async () => {
    installRoutingApi();
    renderRoutingPanel();
    const mode = await screen.findByLabelText("Routing mode");
    const retention = screen.getByLabelText("Decision retention");
    mode.focus();
    expect(mode).toHaveFocus();
    await userEvent.tab();
    expect(retention).toHaveFocus();
  });
});
