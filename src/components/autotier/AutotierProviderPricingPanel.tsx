import {
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import {
  QueryClient,
  QueryClientContext,
  QueryClientProvider,
} from "@tanstack/react-query";
import { Loader2, Plus, Trash2 } from "lucide-react";
import {
  type AutotierProviderModelPricing,
  type AutotierProviderModelPricingInput,
} from "@/lib/api/autotier";
import {
  useAutotierProviderModelPricing,
  useDeleteAutotierProviderModelPricing,
  useUpsertAutotierProviderModelPricing,
} from "@/lib/query/autotier";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type PricingDraft = AutotierProviderModelPricingInput;
const NEW_ROW_KEY = "__new__";
const EMPTY_PRICING: AutotierProviderModelPricing[] = [];

const emptyDraft = (providerId: string): PricingDraft => ({
  provider_id: providerId,
  model_id: "",
  display_name: "",
  input_cost_per_million: "0",
  output_cost_per_million: "0",
  cache_read_cost_per_million: "0",
  cache_creation_cost_per_million: "0",
  price_source: "manual",
});

function draftFromRow(row: AutotierProviderModelPricing): PricingDraft {
  return {
    provider_id: row.provider_id,
    model_id: row.model_id,
    display_name: row.display_name,
    input_cost_per_million: row.input_cost_per_million,
    output_cost_per_million: row.output_cost_per_million,
    cache_read_cost_per_million: row.cache_read_cost_per_million,
    cache_creation_cost_per_million: row.cache_creation_cost_per_million,
    price_source: row.price_source,
  };
}

function PricingField({
  label,
  value,
  onChange,
  inputMode,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  inputMode?: "decimal" | "text";
}) {
  return (
    <div className="space-y-1">
      <Label className="text-xs">{label}</Label>
      <Input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        inputMode={inputMode}
        autoComplete="off"
        spellCheck={false}
      />
    </div>
  );
}

function EnsureQueryClient({ children }: { children: ReactNode }) {
  const parent = useContext(QueryClientContext);
  const [fallback] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0 },
          mutations: { retry: false, gcTime: 0 },
        },
      }),
  );
  if (parent) return <>{children}</>;
  return (
    <QueryClientProvider client={fallback}>{children}</QueryClientProvider>
  );
}

export function AutotierProviderPricingPanel({
  providerId,
}: {
  providerId: string;
}) {
  return (
    <EnsureQueryClient>
      <AutotierProviderPricingPanelInner providerId={providerId} />
    </EnsureQueryClient>
  );
}

function AutotierProviderPricingPanelInner({
  providerId,
}: {
  providerId: string;
}) {
  const { t } = useTranslation();
  const pricingQuery = useAutotierProviderModelPricing(providerId);
  const upsert = useUpsertAutotierProviderModelPricing();
  const remove = useDeleteAutotierProviderModelPricing();
  const [drafts, setDrafts] = useState<Record<string, PricingDraft>>({});
  const [newRowOpen, setNewRowOpen] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);

  const rows = pricingQuery.data ?? EMPTY_PRICING;
  const rowKeys = useMemo(() => rows.map((row) => row.model_id), [rows]);

  useEffect(() => {
    setDrafts((current) => {
      const next: Record<string, PricingDraft> = {};
      for (const row of rows) {
        next[row.model_id] = current[row.model_id] ?? draftFromRow(row);
      }
      if (newRowOpen) {
        next[NEW_ROW_KEY] = current[NEW_ROW_KEY] ?? emptyDraft(providerId);
      }
      return next;
    });
  }, [newRowOpen, providerId, rows]);

  useEffect(() => {
    setDrafts({});
    setNewRowOpen(false);
    setFeedback(null);
  }, [providerId]);

  const updateDraft = (key: string, patch: Partial<PricingDraft>) => {
    setDrafts((current) => ({
      ...current,
      [key]: { ...(current[key] ?? emptyDraft(providerId)), ...patch },
    }));
  };

  const saveDraft = async (key: string) => {
    const draft = drafts[key];
    if (!draft || !draft.model_id.trim()) return;
    setFeedback(null);
    try {
      const saved = await upsert.mutateAsync({
        ...draft,
        provider_id: providerId,
        model_id: draft.model_id.trim(),
        display_name: draft.display_name.trim(),
        price_source: draft.price_source?.trim() || "manual",
      });
      setDrafts((current) => ({
        ...current,
        [saved.model_id]: draftFromRow(saved),
        [NEW_ROW_KEY]: emptyDraft(providerId),
      }));
      if (key === NEW_ROW_KEY) setNewRowOpen(false);
      setFeedback(
        t("autotier.pricing.saved", {
          defaultValue: "Pricing snapshot saved.",
        }),
      );
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    }
  };

  const deleteRow = async (modelId: string) => {
    setFeedback(null);
    try {
      await remove.mutateAsync({ providerId, modelId });
      setFeedback(
        t("autotier.pricing.deleted", {
          defaultValue: "Pricing snapshot deleted.",
        }),
      );
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    }
  };

  const renderEditor = (key: string, isNew = false) => {
    const draft = drafts[key] ?? emptyDraft(providerId);
    const canSave = draft.model_id.trim().length > 0;
    return (
      <div
        key={key}
        className="space-y-3 rounded-md border border-border-default p-4"
        data-testid={`autotier-pricing-row-${isNew ? "new" : key}`}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <PricingField
            label={t("autotier.pricing.modelId", { defaultValue: "Model ID" })}
            value={draft.model_id}
            onChange={(value) => updateDraft(key, { model_id: value })}
          />
          <PricingField
            label={t("autotier.pricing.displayName", {
              defaultValue: "Display name",
            })}
            value={draft.display_name}
            onChange={(value) => updateDraft(key, { display_name: value })}
          />
        </div>
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          <PricingField
            label={t("autotier.pricing.input", {
              defaultValue: "Input / 1M",
            })}
            value={draft.input_cost_per_million}
            inputMode="decimal"
            onChange={(value) =>
              updateDraft(key, { input_cost_per_million: value })
            }
          />
          <PricingField
            label={t("autotier.pricing.output", {
              defaultValue: "Output / 1M",
            })}
            value={draft.output_cost_per_million}
            inputMode="decimal"
            onChange={(value) =>
              updateDraft(key, { output_cost_per_million: value })
            }
          />
          <PricingField
            label={t("autotier.pricing.cacheRead", {
              defaultValue: "Cache read / 1M",
            })}
            value={draft.cache_read_cost_per_million}
            inputMode="decimal"
            onChange={(value) =>
              updateDraft(key, { cache_read_cost_per_million: value })
            }
          />
          <PricingField
            label={t("autotier.pricing.cacheCreation", {
              defaultValue: "Cache write / 1M",
            })}
            value={draft.cache_creation_cost_per_million}
            inputMode="decimal"
            onChange={(value) =>
              updateDraft(key, { cache_creation_cost_per_million: value })
            }
          />
        </div>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
          <div className="flex-1">
            <PricingField
              label={t("autotier.pricing.source", {
                defaultValue: "Price source",
              })}
              value={draft.price_source ?? "manual"}
              onChange={(value) => updateDraft(key, { price_source: value })}
            />
          </div>
          <Button
            type="button"
            disabled={!canSave || upsert.isPending}
            onClick={() => void saveDraft(key)}
          >
            {upsert.isPending &&
            upsert.variables?.model_id === draft.model_id ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : null}
            {t("common.save", { defaultValue: "Save" })}
          </Button>
          {isNew ? (
            <Button
              type="button"
              variant="outline"
              onClick={() => setNewRowOpen(false)}
            >
              {t("common.cancel", { defaultValue: "Cancel" })}
            </Button>
          ) : (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label={t("autotier.pricing.delete", {
                defaultValue: "Delete pricing snapshot",
              })}
              disabled={remove.isPending}
              onClick={() => void deleteRow(key)}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          )}
        </div>
        {!isNew && rows.find((row) => row.model_id === key)?.observed_at ? (
          <p className="text-xs text-muted-foreground">
            {t("autotier.pricing.observedAt", {
              defaultValue: "Observed {{date}}",
              date: new Date(
                rows.find((row) => row.model_id === key)?.observed_at ?? 0,
              ).toLocaleString(),
            })}
          </p>
        ) : null}
      </div>
    );
  };

  return (
    <Card
      className="mt-8"
      data-testid="autotier-provider-pricing-panel"
      aria-labelledby="autotier-pricing-heading"
    >
      <CardHeader>
        <CardTitle id="autotier-pricing-heading">
          {t("autotier.pricing.title", {
            defaultValue: "Provider pricing snapshots",
          })}
        </CardTitle>
        <CardDescription>
          {t("autotier.pricing.subtitle", {
            defaultValue:
              "Prices are scoped to this Provider and take precedence over the global model table. Enter costs per one million tokens.",
          })}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {pricingQuery.isLoading ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("autotier.pricing.loading", {
              defaultValue: "Loading pricing…",
            })}
          </div>
        ) : null}
        {pricingQuery.error ? (
          <Alert variant="destructive">
            <AlertTitle>
              {t("autotier.pricing.errorTitle", {
                defaultValue: "Could not load pricing snapshots",
              })}
            </AlertTitle>
            <AlertDescription>
              {pricingQuery.error instanceof Error
                ? pricingQuery.error.message
                : String(pricingQuery.error)}
            </AlertDescription>
          </Alert>
        ) : null}
        {feedback ? (
          <Alert>
            <AlertDescription>{feedback}</AlertDescription>
          </Alert>
        ) : null}
        {!pricingQuery.isLoading && rows.length === 0 && !newRowOpen ? (
          <p className="text-sm text-muted-foreground">
            {t("autotier.pricing.empty", {
              defaultValue: "No Provider-specific pricing snapshots saved yet.",
            })}
          </p>
        ) : null}
        <div className="space-y-3">
          {rowKeys.map((key) => renderEditor(key))}
          {newRowOpen ? renderEditor(NEW_ROW_KEY, true) : null}
        </div>
        {!newRowOpen ? (
          <Button
            type="button"
            variant="outline"
            onClick={() => setNewRowOpen(true)}
          >
            <Plus className="mr-2 h-4 w-4" />
            {t("autotier.pricing.add", {
              defaultValue: "Add pricing snapshot",
            })}
          </Button>
        ) : null}
      </CardContent>
    </Card>
  );
}
