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
import { Loader2 } from "lucide-react";
import {
  displayCapabilityStatus,
  type AutotierProviderSlot,
} from "@/lib/api/autotier";
import {
  useAutotierProviderSlots,
  useAutotierRequiredSlots,
  useUpsertAutotierProviderSlot,
} from "@/lib/query/autotier";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Label } from "@/components/ui/label";
import {
  AUTOTIER_OPTIONAL_SLOTS,
  duplicateModelGroups,
  INVALID_SLOT_REASON_COPY,
  invalidSlotReasons,
  rowsForSlotUi,
  shouldShowLiveReady,
  slotDisplayLabel,
} from "@/components/autotier/autotierSlotUi";

const LIVE_READY_LABEL = "Live-ready";
const EMPTY_SLOTS: AutotierProviderSlot[] = [];

export interface AutotierProviderSlotsPanelProps {
  providerId: string;
  knownModelIds?: readonly string[];
}

function EnsureQueryClient({ children }: { children: ReactNode }) {
  const parent = useContext(QueryClientContext);
  const [fallback] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: false },
          mutations: { retry: false },
        },
      }),
  );
  if (parent) return <>{children}</>;
  return (
    <QueryClientProvider client={fallback}>{children}</QueryClientProvider>
  );
}

export function AutotierProviderSlotsPanel(
  props: AutotierProviderSlotsPanelProps,
) {
  return (
    <EnsureQueryClient>
      <AutotierProviderSlotsPanelInner {...props} />
    </EnsureQueryClient>
  );
}

function AutotierProviderSlotsPanelInner({
  providerId,
  knownModelIds,
}: AutotierProviderSlotsPanelProps) {
  const { t } = useTranslation();
  const slotsQuery = useAutotierProviderSlots(providerId);
  const requiredQuery = useAutotierRequiredSlots(providerId);
  const upsert = useUpsertAutotierProviderSlot();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [localOptional, setLocalOptional] = useState<string[]>([]);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savingSlot, setSavingSlot] = useState<string | null>(null);

  const slots = slotsQuery.data ?? EMPTY_SLOTS;
  const rows = useMemo(
    () => rowsForSlotUi(providerId, slots, localOptional),
    [providerId, slots, localOptional],
  );

  useEffect(() => {
    setDrafts((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const row of rows) {
        if (next[row.slot] === undefined) {
          next[row.slot] = row.model_id;
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [rows]);

  const displayRows = rows.map((row) => ({
    ...row,
    model_id: drafts[row.slot] ?? row.model_id,
  }));

  const duplicates = duplicateModelGroups(displayRows);
  const missingRequired = requiredQuery.data?.missing ?? [
    "cheap",
    "mid",
    "strong",
  ];
  const requiredComplete = requiredQuery.data?.complete === true;

  const handleSave = async (row: AutotierProviderSlot) => {
    const modelId = (drafts[row.slot] ?? row.model_id).trim();
    if (!modelId) return;
    setSaveError(null);
    setSavingSlot(row.slot);
    try {
      const saved = await upsert.mutateAsync({
        ...row,
        provider_id: providerId,
        model_id: modelId,
      });
      setDrafts((prev) => ({ ...prev, [saved.slot]: saved.model_id }));
      setLocalOptional((prev) => prev.filter((slot) => slot !== saved.slot));
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setSavingSlot(null);
    }
  };

  const isLoading = slotsQuery.isLoading || requiredQuery.isLoading;
  const loadError = slotsQuery.error ?? requiredQuery.error;

  return (
    <Card
      className="mt-8"
      data-testid="autotier-provider-slots-panel"
      aria-labelledby="autotier-slots-heading"
    >
      <CardHeader>
        <CardTitle id="autotier-slots-heading">
          {t("autotier.slots.title", { defaultValue: "AutoTier slots" })}
        </CardTitle>
        <CardDescription>
          {t("autotier.slots.subtitle", {
            defaultValue:
              "Assign Cheap, Mid, and Strong models for this provider. Optional Long Context and Background slots can be added later.",
          })}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <Alert data-testid="autotier-shadow-explainer">
          <AlertTitle>
            {t("autotier.slots.shadowTitle", { defaultValue: "Shadow mode" })}
          </AlertTitle>
          <AlertDescription>
            {t("autotier.slots.shadowBody", {
              defaultValue:
                "Shadow records a candidate for measurement only. It does not change the outbound model or provider.",
            })}
          </AlertDescription>
        </Alert>

        {isLoading ? (
          <div
            role="status"
            className="flex items-center gap-2 text-sm text-muted-foreground"
          >
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            {t("autotier.slots.loading", { defaultValue: "Loading slots…" })}
          </div>
        ) : null}

        {loadError && !isLoading ? (
          <Alert variant="destructive" data-testid="autotier-slots-error">
            <AlertTitle>
              {t("autotier.slots.errorTitle", {
                defaultValue: "Could not load slots",
              })}
            </AlertTitle>
            <AlertDescription className="space-y-3">
              <p>
                {loadError instanceof Error
                  ? loadError.message
                  : String(loadError)}
              </p>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  void slotsQuery.refetch();
                  void requiredQuery.refetch();
                }}
              >
                {t("autotier.slots.retry", { defaultValue: "Retry" })}
              </Button>
            </AlertDescription>
          </Alert>
        ) : null}

        {!isLoading && !loadError ? (
          <>
            {slots.length === 0 ? (
              <p
                className="text-sm text-muted-foreground"
                data-testid="autotier-slots-empty"
              >
                {t("autotier.slots.empty", {
                  defaultValue:
                    "No slots saved yet. Enter a model ID for Cheap, Mid, and Strong, then save each row.",
                })}
              </p>
            ) : null}

            {!requiredComplete ? (
              <p className="text-sm text-muted-foreground">
                {t("autotier.slots.requiredMissing", {
                  defaultValue: "Required slots still missing: {{slots}}.",
                  slots: missingRequired
                    .map((slot) => slotDisplayLabel(slot))
                    .join(", "),
                })}
              </p>
            ) : (
              <p
                className="text-sm text-muted-foreground"
                data-testid="autotier-slots-complete"
              >
                {t("autotier.slots.requiredComplete", {
                  defaultValue:
                    "Required Cheap, Mid, and Strong slots are complete.",
                })}
              </p>
            )}

            {[...duplicates.entries()].map(([modelId, slotNames]) => (
              <Alert key={modelId} data-testid="autotier-duplicate-model">
                <AlertTitle>
                  {t("autotier.slots.duplicateTitle", {
                    defaultValue: "Same model on multiple slots",
                  })}
                </AlertTitle>
                <AlertDescription>
                  {t("autotier.slots.duplicateBody", {
                    defaultValue:
                      "{{model}} is assigned to {{slots}}. The same model can be reused, but these tiers are not actually distinct.",
                    model: modelId,
                    slots: slotNames
                      .map((slot) => slotDisplayLabel(slot))
                      .join(", "),
                  })}
                </AlertDescription>
              </Alert>
            ))}

            {saveError ? (
              <Alert variant="destructive">
                <AlertDescription>{saveError}</AlertDescription>
              </Alert>
            ) : null}

            <ol className="space-y-4">
              {displayRows.map((row) => {
                const reasons = invalidSlotReasons({
                  slot: row.slot,
                  model_id: row.model_id,
                  capability_status: row.capability_status,
                  knownModelIds,
                });
                const inputId = `autotier-slot-${row.slot}-model`;
                const errorId = `autotier-slot-${row.slot}-errors`;
                const canSave = row.model_id.trim().length > 0;
                return (
                  <li
                    key={row.slot}
                    className="space-y-2 rounded-md border border-border-default p-4"
                    data-testid={`autotier-slot-row-${row.slot}`}
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <h3 className="text-sm font-medium">
                        {slotDisplayLabel(row.slot)}
                      </h3>
                      {row.created_at > 0 ? (
                        <Badge variant="outline">
                          {displayCapabilityStatus(row.capability_status)}
                        </Badge>
                      ) : (
                        <Badge variant="secondary">
                          {t("autotier.slots.draft", { defaultValue: "Draft" })}
                        </Badge>
                      )}
                      {shouldShowLiveReady(row.capability_status) ? (
                        <Badge data-testid="autotier-live-ready">
                          {LIVE_READY_LABEL}
                        </Badge>
                      ) : null}
                    </div>
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
                      <div className="flex-1 space-y-1.5">
                        <Label htmlFor={inputId}>
                          {t("autotier.slots.modelLabel", {
                            defaultValue: `${slotDisplayLabel(row.slot)} model`,
                            slot: slotDisplayLabel(row.slot),
                          })}
                        </Label>
                        <ImeSafeInput
                          id={inputId}
                          value={drafts[row.slot] ?? ""}
                          onValueChange={(value) =>
                            setDrafts((prev) => ({
                              ...prev,
                              [row.slot]: value,
                            }))
                          }
                          aria-invalid={reasons.length > 0}
                          aria-describedby={
                            reasons.length > 0 ? errorId : undefined
                          }
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </div>
                      <Button
                        type="button"
                        disabled={!canSave || savingSlot === row.slot}
                        aria-label={t("autotier.slots.saveSlot", {
                          defaultValue: `Save ${slotDisplayLabel(row.slot)} slot`,
                          slot: slotDisplayLabel(row.slot),
                        })}
                        onClick={() => {
                          void handleSave(row);
                        }}
                      >
                        {savingSlot === row.slot ? (
                          <Loader2
                            className="h-4 w-4 animate-spin"
                            aria-hidden="true"
                          />
                        ) : (
                          t("common.save", { defaultValue: "Save" })
                        )}
                      </Button>
                    </div>
                    {row.created_at > 0 ? (
                      <dl className="grid gap-1 text-xs text-muted-foreground sm:grid-cols-2">
                        <div>
                          <dt className="inline font-medium">
                            {t("autotier.slots.pricingSource", {
                              defaultValue: "Pricing source",
                            })}
                            {": "}
                          </dt>
                          <dd className="inline">
                            {row.pricing_source ||
                              t("autotier.slots.sourceUnknown", {
                                defaultValue: "unknown",
                              })}
                          </dd>
                        </div>
                        <div>
                          <dt className="inline font-medium">
                            {t("autotier.slots.capabilitySource", {
                              defaultValue: "Capability source",
                            })}
                            {": "}
                          </dt>
                          <dd className="inline">
                            {row.capability_source ||
                              t("autotier.slots.sourceUnknown", {
                                defaultValue: "unknown",
                              })}
                          </dd>
                        </div>
                      </dl>
                    ) : null}
                    {reasons.length > 0 ? (
                      <ul
                        id={errorId}
                        className="list-disc space-y-1 pl-5 text-sm text-destructive"
                      >
                        {reasons.map((reason) => (
                          <li key={reason}>
                            {t(`autotier.slots.invalid.${reason}`, {
                              defaultValue: INVALID_SLOT_REASON_COPY[reason],
                            })}
                          </li>
                        ))}
                      </ul>
                    ) : null}
                  </li>
                );
              })}
            </ol>

            <div className="flex flex-wrap gap-2">
              {AUTOTIER_OPTIONAL_SLOTS.filter(
                (slot) => !displayRows.some((row) => row.slot === slot),
              ).map((slot) => (
                <Button
                  key={slot}
                  type="button"
                  variant="outline"
                  onClick={() =>
                    setLocalOptional((prev) =>
                      prev.includes(slot) ? prev : [...prev, slot],
                    )
                  }
                >
                  {t("autotier.slots.addOptional", {
                    defaultValue: `Add ${slotDisplayLabel(slot)} slot`,
                    slot: slotDisplayLabel(slot),
                  })}
                </Button>
              ))}
            </div>
          </>
        ) : null}
      </CardContent>
    </Card>
  );
}
