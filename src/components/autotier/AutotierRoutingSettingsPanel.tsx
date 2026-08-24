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
import { Eye, Loader2, Route, Save, Trash2 } from "lucide-react";
import { toast } from "sonner";
import {
  AUTOTIER_MODES_V01,
  AUTOTIER_RETENTION_DAYS,
  displayAutotierMode,
  effectiveAutotierMode,
  type AutotierModeV01,
  type AutotierRetentionDays,
  type AutotierSaveConfigInput,
} from "@/lib/api/autotier";
import {
  useAutotierRoutingConfig,
  useClearAutotierDecisions,
  useImportAutotierLegacyData,
  useAutotierLegacyStatus,
  usePruneAutotierDecisions,
  useSaveAutotierRoutingConfig,
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
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export type ForcedCandidateChoice = "none" | "cheap" | "mid" | "strong";

export function forcedChoiceFromMode(mode: string): ForcedCandidateChoice {
  switch (mode) {
    case "forced_cheap":
      return "cheap";
    case "forced_mid":
      return "mid";
    case "forced_strong":
      return "strong";
    default:
      return "none";
  }
}

export function buildRoutingSaveMode(
  baseMode: AutotierModeV01,
  _forced: ForcedCandidateChoice,
): AutotierModeV01 {
  return baseMode;
}

export function advisoryCandidateFromChoice(
  choice: ForcedCandidateChoice,
): "cheap" | "mid" | "strong" | null {
  return choice === "none" ? null : choice;
}

export function policyHintStatusV01(): "notConnected" {
  return "notConnected";
}

export function canaryDataGateMetV01(): boolean {
  return false;
}

export function shouldShowLiveRoutingUi(): boolean {
  return false;
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

export function AutotierRoutingSettingsPanel() {
  return (
    <EnsureQueryClient>
      <AutotierRoutingSettingsPanelInner />
    </EnsureQueryClient>
  );
}

function AutotierRoutingSettingsPanelInner() {
  const { t } = useTranslation();
  const configQuery = useAutotierRoutingConfig();
  const saveConfig = useSaveAutotierRoutingConfig();
  const clearDecisions = useClearAutotierDecisions();
  const pruneDecisions = usePruneAutotierDecisions();
  const legacyStatus = useAutotierLegacyStatus();
  const importLegacy = useImportAutotierLegacyData();
  const [baseMode, setBaseMode] = useState<AutotierModeV01>("shadow");
  const [retentionDays, setRetentionDays] = useState<AutotierRetentionDays>(30);
  const [forcedCandidate, setForcedCandidate] =
    useState<ForcedCandidateChoice>("none");
  const [visionCopilotEnabled, setVisionCopilotEnabled] = useState(false);
  const [visionCopilotModel, setVisionCopilotModel] = useState("");
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [lastDegradedFrom, setLastDegradedFrom] = useState<string | null>(null);

  useEffect(() => {
    if (!configQuery.data) return;
    const effective = effectiveAutotierMode(configQuery.data.mode);
    setBaseMode(effective);
    setRetentionDays(configQuery.data.retention_days as AutotierRetentionDays);
    const degraded = configQuery.data.degraded_from;
    setForcedCandidate(
      configQuery.data.advisory_candidate === "cheap" ||
        configQuery.data.advisory_candidate === "mid" ||
        configQuery.data.advisory_candidate === "strong"
        ? configQuery.data.advisory_candidate
        : degraded
          ? forcedChoiceFromMode(degraded)
          : forcedChoiceFromMode(configQuery.data.mode),
    );
    setLastDegradedFrom(degraded);
    setVisionCopilotEnabled(configQuery.data.vision_copilot_enabled ?? false);
    setVisionCopilotModel(configQuery.data.vision_copilot_model ?? "");
  }, [configQuery.data]);

  const hintStatus = policyHintStatusV01();
  const canaryMet = canaryDataGateMetV01();
  const config = configQuery.data;

  const modeDescription = useMemo(() => {
    if (baseMode === "shadow") {
      return t("autotier.routing.modeDescriptionShadow");
    }
    return t("autotier.routing.modeDescriptionOff");
  }, [baseMode, t]);

  const handleSave = async () => {
    const saveMode = buildRoutingSaveMode(baseMode, forcedCandidate);
    try {
      const saved = await saveConfig.mutateAsync({
        mode: saveMode as AutotierSaveConfigInput["mode"],
        advisory_candidate: advisoryCandidateFromChoice(forcedCandidate),
        retention_days: retentionDays,
        vision_copilot_enabled: visionCopilotEnabled,
        vision_copilot_model: visionCopilotModel,
        vision_text_only_models: [],
      });
      setLastDegradedFrom(saved.degraded_from);
      toast.success(t("autotier.routing.saveSuccess"));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleClear = async () => {
    try {
      await clearDecisions.mutateAsync();
      toast.success(t("autotier.routing.clearSuccess"));
      setShowClearConfirm(false);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handlePrune = async () => {
    try {
      const count = await pruneDecisions.mutateAsync(retentionDays);
      toast.success(t("autotier.routing.pruneSuccess", { count }));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleImportLegacy = async () => {
    try {
      const result = await importLegacy.mutateAsync();
      toast.success(
        t("autotier.routing.legacyImportSuccess", {
          path: result.imported_to,
        }),
      );
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const showLegacyImport = legacyStatus.data?.legacy_db_exists;

  const isLoading = configQuery.isLoading;
  const loadError = configQuery.error;

  return (
    <Card
      data-testid="autotier-routing-settings-panel"
      aria-labelledby="autotier-routing-heading"
    >
      <CardHeader>
        <CardTitle
          id="autotier-routing-heading"
          className="flex items-center gap-2 text-xl"
        >
          <Route className="h-5 w-5" aria-hidden="true" />
          {t("autotier.routing.title")}
        </CardTitle>
        <CardDescription>{t("autotier.routing.subtitle")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        {shouldShowLiveRoutingUi() ? null : (
          <Alert data-testid="autotier-no-live-banner">
            <AlertTitle>{t("autotier.routing.noLiveTitle")}</AlertTitle>
            <AlertDescription>
              {t("autotier.routing.noLiveBody")}
            </AlertDescription>
          </Alert>
        )}

        {showLegacyImport ? (
          <Alert data-testid="autotier-legacy-import-banner">
            <AlertTitle>{t("autotier.routing.legacyImportTitle")}</AlertTitle>
            <AlertDescription className="space-y-3">
              <p>{t("autotier.routing.legacyImportBody")}</p>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                disabled={importLegacy.isPending}
                onClick={() => void handleImportLegacy()}
              >
                {importLegacy.isPending
                  ? t("autotier.routing.legacyImportRunning")
                  : t("autotier.routing.legacyImportAction")}
              </Button>
            </AlertDescription>
          </Alert>
        ) : null}

        {isLoading ? (
          <div
            role="status"
            className="flex items-center gap-2 text-sm text-muted-foreground"
          >
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            {t("autotier.routing.loading")}
          </div>
        ) : null}

        {loadError && !isLoading ? (
          <Alert variant="destructive" data-testid="autotier-routing-error">
            <AlertTitle>{t("autotier.routing.errorTitle")}</AlertTitle>
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
                  void configQuery.refetch();
                }}
              >
                {t("autotier.routing.retry")}
              </Button>
            </AlertDescription>
          </Alert>
        ) : null}

        {!isLoading && !loadError ? (
          <>
            <div className="space-y-2">
              <Label htmlFor="autotier-routing-mode">
                {t("autotier.routing.modeLabel")}
              </Label>
              <Select
                value={baseMode}
                onValueChange={(value) => setBaseMode(value as AutotierModeV01)}
              >
                <SelectTrigger id="autotier-routing-mode">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {AUTOTIER_MODES_V01.map((mode) => (
                    <SelectItem key={mode} value={mode}>
                      {mode === "off"
                        ? t("autotier.routing.modeOff")
                        : t("autotier.routing.modeShadow")}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-sm text-muted-foreground">{modeDescription}</p>
              {config ? (
                <p className="text-xs text-muted-foreground">
                  {t("autotier.routing.modeLabel")}:{" "}
                  {displayAutotierMode(config.mode) === "unknown"
                    ? t("common.unknown")
                    : displayAutotierMode(config.mode)}
                </p>
              ) : null}
            </div>

            <div className="space-y-2">
              <Label htmlFor="autotier-retention-days">
                {t("autotier.routing.retentionLabel")}
              </Label>
              <Select
                value={String(retentionDays)}
                onValueChange={(value) =>
                  setRetentionDays(Number(value) as AutotierRetentionDays)
                }
              >
                <SelectTrigger id="autotier-retention-days">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {AUTOTIER_RETENTION_DAYS.map((days) => (
                    <SelectItem key={days} value={String(days)}>
                      {t("autotier.routing.retentionDays", { days })}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-sm text-muted-foreground">
                {t("autotier.routing.retentionDescription")}
              </p>
            </div>

            <div className="space-y-3 rounded-md border border-border-default p-4">
              <div className="flex items-center justify-between gap-4">
                <div className="flex items-start gap-2">
                  <Eye className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                  <div>
                    <Label htmlFor="autotier-vision-copilot">
                      {t("autotier.routing.visionCopilotLabel", {
                        defaultValue: "图片助手",
                      })}
                    </Label>
                    <p className="text-sm text-muted-foreground">
                      {t("autotier.routing.visionCopilotDescription", {
                        defaultValue:
                          "当前模型明确看不了图片时，先让已声明支持图片的模型转成文字，再交回当前模型回答。未知模型不自动改写。",
                      })}
                    </p>
                  </div>
                </div>
                <Switch
                  id="autotier-vision-copilot"
                  checked={visionCopilotEnabled}
                  onCheckedChange={setVisionCopilotEnabled}
                  aria-label={t("autotier.routing.visionCopilotLabel", {
                    defaultValue: "图片助手",
                  })}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="autotier-vision-copilot-model">
                  {t("autotier.routing.visionCopilotModelLabel", {
                    defaultValue: "图片助手模型（可留空自动找）",
                  })}
                </Label>
                <Input
                  id="autotier-vision-copilot-model"
                  value={visionCopilotModel}
                  onChange={(event) =>
                    setVisionCopilotModel(event.target.value)
                  }
                  placeholder={t(
                    "autotier.routing.visionCopilotModelPlaceholder",
                    {
                      defaultValue: "留空：从供应商模型声明中自动选择",
                    },
                  )}
                  disabled={!visionCopilotEnabled}
                />
              </div>
            </div>

            <fieldset
              className="space-y-3 rounded-md border border-border-default p-4"
              disabled={baseMode !== "shadow"}
            >
              <legend className="px-1 text-sm font-medium">
                {t("autotier.routing.forcedCandidateTitle")}
              </legend>
              <p className="text-sm text-muted-foreground">
                {t("autotier.routing.forcedCandidateDescription")}
              </p>
              <Select
                value={forcedCandidate}
                onValueChange={(value) =>
                  setForcedCandidate(value as ForcedCandidateChoice)
                }
              >
                <SelectTrigger
                  id="autotier-forced-candidate"
                  aria-label={t("autotier.routing.forcedCandidateTitle")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">
                    {t("autotier.routing.forcedCandidateNone")}
                  </SelectItem>
                  <SelectItem value="cheap">
                    {t("autotier.routing.forcedCandidateCheap")}
                  </SelectItem>
                  <SelectItem value="mid">
                    {t("autotier.routing.forcedCandidateMid")}
                  </SelectItem>
                  <SelectItem value="strong">
                    {t("autotier.routing.forcedCandidateStrong")}
                  </SelectItem>
                </SelectContent>
              </Select>
              {forcedCandidate !== "none" ? (
                <Alert data-testid="autotier-forced-advisory">
                  <AlertTitle>
                    {t("autotier.routing.forcedCandidateAdvisory")}
                  </AlertTitle>
                  <AlertDescription>
                    {t("autotier.routing.forcedCandidateNoExecute")}
                  </AlertDescription>
                </Alert>
              ) : null}
            </fieldset>

            {lastDegradedFrom ? (
              <Alert data-testid="autotier-degraded-warning">
                <AlertDescription>
                  {t("autotier.routing.degradedWarning", {
                    from: lastDegradedFrom,
                  })}
                </AlertDescription>
              </Alert>
            ) : null}

            <Alert data-testid="autotier-privacy-copy">
              <AlertTitle>{t("autotier.routing.privacyTitle")}</AlertTitle>
              <AlertDescription>
                {t("autotier.routing.privacyBody")}
              </AlertDescription>
            </Alert>

            <div className="flex items-center justify-between rounded-md border border-border-default px-4 py-3">
              <div>
                <p className="text-sm font-medium">
                  {t("autotier.routing.rawPromptLabel")}
                </p>
                <p className="text-xs text-muted-foreground">
                  {t("autotier.routing.rawPromptLocked")}
                </p>
              </div>
              <Badge variant="secondary">
                {t("autotier.routing.rawPromptLocked")}
              </Badge>
            </div>

            {config ? (
              <dl
                className="grid gap-2 rounded-md border border-border-default p-4 text-sm"
                data-testid="autotier-version-stamps"
              >
                <div className="font-medium">
                  {t("autotier.routing.versionsTitle")}
                </div>
                <div>
                  <dt className="inline text-muted-foreground">
                    {t("autotier.routing.classifierVersion")}:{" "}
                  </dt>
                  <dd className="inline">{config.classifier_version}</dd>
                </div>
                <div>
                  <dt className="inline text-muted-foreground">
                    {t("autotier.routing.featureVersion")}:{" "}
                  </dt>
                  <dd className="inline">{config.feature_version}</dd>
                </div>
                <div>
                  <dt className="inline text-muted-foreground">
                    {t("autotier.routing.policyVersion")}:{" "}
                  </dt>
                  <dd className="inline">{config.policy_version}</dd>
                </div>
                <div>
                  <dt className="inline text-muted-foreground">
                    {t("autotier.routing.capabilityTableVersion")}:{" "}
                  </dt>
                  <dd className="inline">{config.capability_table_version}</dd>
                </div>
                <div>
                  <dt className="inline text-muted-foreground">
                    {t("autotier.routing.costModelVersion")}:{" "}
                  </dt>
                  <dd className="inline">{config.cost_model_version}</dd>
                </div>
                <div>
                  <dt className="inline text-muted-foreground">
                    {t("autotier.routing.cacheStatsVersion")}:{" "}
                  </dt>
                  <dd className="inline">{config.cache_stats_version}</dd>
                </div>
              </dl>
            ) : null}

            <div className="grid gap-4 sm:grid-cols-2">
              <div className="rounded-md border border-border-default p-4">
                <p className="text-sm font-medium">
                  {t("autotier.routing.hintTitle")}
                </p>
                <Badge variant="outline" className="mt-2">
                  {t(`autotier.routing.hintStatus.${hintStatus}`)}
                </Badge>
              </div>
              <div
                className="rounded-md border border-border-default p-4"
                data-testid="autotier-canary-gate"
              >
                <p className="text-sm font-medium">
                  {t("autotier.routing.canaryGateTitle")}
                </p>
                <Badge
                  variant={canaryMet ? "default" : "secondary"}
                  className="mt-2"
                >
                  {canaryMet
                    ? t("common.enabled")
                    : t("autotier.routing.canaryGateNotMet")}
                </Badge>
                <p className="mt-2 text-xs text-muted-foreground">
                  {t("autotier.routing.canaryGateDescription")}
                </p>
              </div>
            </div>

            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                onClick={() => {
                  void handleSave();
                }}
                disabled={saveConfig.isPending}
              >
                {saveConfig.isPending ? (
                  <Loader2
                    className="h-4 w-4 animate-spin"
                    aria-hidden="true"
                  />
                ) : (
                  <Save className="h-4 w-4" aria-hidden="true" />
                )}
                {t("autotier.routing.save")}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  void handlePrune();
                }}
                disabled={pruneDecisions.isPending}
              >
                {t("autotier.routing.pruneDecisions")}
              </Button>
              <Button
                type="button"
                variant="destructive"
                onClick={() => setShowClearConfirm(true)}
                disabled={clearDecisions.isPending}
              >
                <Trash2 className="h-4 w-4" aria-hidden="true" />
                {t("autotier.routing.clearDecisions")}
              </Button>
            </div>
          </>
        ) : null}
      </CardContent>

      <ConfirmDialog
        isOpen={showClearConfirm}
        variant="destructive"
        title={t("autotier.routing.clearConfirmTitle")}
        message={t("autotier.routing.clearConfirmBody")}
        confirmText={t("autotier.routing.clearConfirmAction")}
        onConfirm={() => {
          void handleClear();
        }}
        onCancel={() => setShowClearConfirm(false)}
      />
    </Card>
  );
}
