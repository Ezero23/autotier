import {
  useContext,
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
import { ChevronLeft, ChevronRight, ClipboardList, Download, Loader2, Play, BarChart3 } from "lucide-react";
import { toast } from "sonner";
import { settingsApi } from "@/lib/api/settings";
import {
  AUTOTIER_DECISION_LABEL_REASONS,
  AUTOTIER_DECISION_LABELS,
  displayDecisionLabel,
  displaySlot,
  parseJsonStringArray,
  type AutotierDecisionLabel,
  type AutotierDecisionLabelReason,
  type AutotierDecisionListItem,
  type AutotierDecisionQueryFilter,
} from "@/lib/api/autotier";
import {
  useAutotierDecisionDetail,
  useAutotierDecisions,
  useEvaluateAutotierExport,
  useExportAutotierDecisions,
  useReplayAutotierExport,
  useUpsertAutotierDecisionLabel,
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

const PAGE_SIZE = 20;

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

export function AutotierDecisionsPanel() {
  return (
    <EnsureQueryClient>
      <AutotierDecisionsPanelInner />
    </EnsureQueryClient>
  );
}

function formatModel(value: string | null | undefined, empty: string): string {
  if (!value || !value.trim()) return empty;
  return value;
}

function formatProvider(value: string | null | undefined, empty: string): string {
  if (!value || !value.trim()) return empty;
  return value;
}

function formatTimestamp(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "—";
  return new Date(ms).toLocaleString();
}

function formatCostRange(
  low: string | null | undefined,
  base: string | null | undefined,
  high: string | null | undefined,
  empty: string,
): string {
  if (!low && !base && !high) return empty;
  return `${low ?? "—"} / ${base ?? "—"} / ${high ?? "—"}`;
}

function RouteGroup({
  title,
  model,
  provider,
  empty,
}: {
  title: string;
  model: string;
  provider: string;
  empty: string;
}) {
  return (
    <div className="rounded-lg border border-border/60 p-3 space-y-1">
      <p className="text-xs font-medium text-muted-foreground">{title}</p>
      <p className="text-sm font-mono break-all">{model}</p>
      <p className="text-xs text-muted-foreground break-all">{provider}</p>
    </div>
  );
}

function AutotierDecisionsPanelInner() {
  const { t } = useTranslation();
  const [offset, setOffset] = useState(0);
  const [appType, setAppType] = useState<string>("all");
  const [completeFilter, setCompleteFilter] = useState<string>("all");
  const [labelFilter, setLabelFilter] = useState<string>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draftLabel, setDraftLabel] = useState<AutotierDecisionLabel>("correct");
  const [draftReason, setDraftReason] = useState<
    AutotierDecisionLabelReason | "none"
  >("none");
  const [draftNote, setDraftNote] = useState("");

  const filter = useMemo<AutotierDecisionQueryFilter>(() => {
    const next: AutotierDecisionQueryFilter = {
      limit: PAGE_SIZE,
      offset,
    };
    if (appType !== "all") next.app_type = appType;
    if (completeFilter === "complete") next.is_complete = true;
    if (completeFilter === "incomplete") next.is_complete = false;
    if (labelFilter === "labeled") next.has_label = true;
    if (labelFilter === "unlabeled") next.has_label = false;
    return next;
  }, [appType, completeFilter, labelFilter, offset]);

  const listQuery = useAutotierDecisions(filter);
  const detailQuery = useAutotierDecisionDetail(selectedId);
  const upsertLabel = useUpsertAutotierDecisionLabel();
  const exportDecisions = useExportAutotierDecisions();
  const replayExport = useReplayAutotierExport();
  const evaluateExport = useEvaluateAutotierExport();
  const [toolSummary, setToolSummary] = useState<string | null>(null);

  const pickExportDir = async (): Promise<string | null> => {
    const dir = await settingsApi.pickDirectory();
    if (!dir) return null;
    return dir;
  };

  const handleExport = async () => {
    try {
      const dir = await pickExportDir();
      if (!dir) return;
      const result = await exportDecisions.mutateAsync(dir);
      setToolSummary(
        t("autotier.decisions.exportSuccess", {
          count: result.manifest.decision_count,
          dir: result.output_dir,
        }),
      );
      toast.success(t("autotier.decisions.exportSuccessShort"));
    } catch (error) {
      toast.error(String(error));
    }
  };

  const handleReplay = async () => {
    try {
      const dir = await pickExportDir();
      if (!dir) return;
      const report = await replayExport.mutateAsync(dir);
      setToolSummary(
        t("autotier.decisions.replaySuccess", {
          matched: report.matched,
          replayed: report.replayed,
          mismatches: report.mismatches.length,
        }),
      );
      toast.success(t("autotier.decisions.replaySuccessShort"));
    } catch (error) {
      toast.error(String(error));
    }
  };

  const handleEvaluate = async () => {
    try {
      const dir = await pickExportDir();
      if (!dir) return;
      const report = await evaluateExport.mutateAsync(dir);
      const warning =
        report.metrics.warnings[0] ??
        t("autotier.decisions.evalNoWarnings");
      setToolSummary(
        t("autotier.decisions.evalSuccess", {
          holdout: report.metrics.holdout_count,
          recall: (report.metrics.strong_recall * 100).toFixed(1),
          warning,
        }),
      );
      toast.success(t("autotier.decisions.evalSuccessShort"));
    } catch (error) {
      toast.error(String(error));
    }
  };

  const toolsBusy =
    exportDecisions.isPending ||
    replayExport.isPending ||
    evaluateExport.isPending;

  const emptyValue = t("autotier.decisions.emptyValue");
  const items = listQuery.data?.items ?? [];
  const total = listQuery.data?.total ?? 0;
  const canPrev = offset > 0;
  const canNext = offset + PAGE_SIZE < total;

  const openDetail = (item: AutotierDecisionListItem) => {
    setSelectedId(item.decision_id);
    if (item.user_label) {
      const label = displayDecisionLabel(item.user_label);
      if (label !== "unknown") setDraftLabel(label);
    }
  };

  const saveLabel = async () => {
    if (!selectedId) return;
    try {
      await upsertLabel.mutateAsync({
        decision_id: selectedId,
        label: draftLabel,
        reason: draftReason === "none" ? null : draftReason,
        note: draftNote.trim() || null,
      });
      toast.success(t("autotier.decisions.labelSaved"));
    } catch (error) {
      toast.error(String(error));
    }
  };

  if (listQuery.isLoading) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground py-8 justify-center">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t("autotier.decisions.loading")}
      </div>
    );
  }

  if (listQuery.isError) {
    return (
      <Alert variant="destructive">
        <AlertTitle>{t("autotier.decisions.errorTitle")}</AlertTitle>
        <AlertDescription>
          {String(listQuery.error)}
          <Button
            variant="outline"
            size="sm"
            className="mt-3"
            onClick={() => void listQuery.refetch()}
          >
            {t("autotier.decisions.retry")}
          </Button>
        </AlertDescription>
      </Alert>
    );
  }

  const detail = detailQuery.data;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <ClipboardList className="h-4 w-4" />
            {t("autotier.decisions.title")}
          </CardTitle>
          <CardDescription>{t("autotier.decisions.subtitle")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap gap-2" data-testid="autotier-data-tools">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={toolsBusy}
              onClick={() => void handleExport()}
            >
              {exportDecisions.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin mr-1" />
              ) : (
                <Download className="h-4 w-4 mr-1" />
              )}
              {t("autotier.decisions.exportButton")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={toolsBusy}
              onClick={() => void handleReplay()}
            >
              {replayExport.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin mr-1" />
              ) : (
                <Play className="h-4 w-4 mr-1" />
              )}
              {t("autotier.decisions.replayButton")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={toolsBusy}
              onClick={() => void handleEvaluate()}
            >
              {evaluateExport.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin mr-1" />
              ) : (
                <BarChart3 className="h-4 w-4 mr-1" />
              )}
              {t("autotier.decisions.evalButton")}
            </Button>
          </div>
          {toolSummary ? (
            <Alert data-testid="autotier-data-tools-summary">
              <AlertDescription>{toolSummary}</AlertDescription>
            </Alert>
          ) : null}
          <div className="flex flex-wrap gap-3">
            <div className="space-y-1">
              <Label>{t("autotier.decisions.filterAppType")}</Label>
              <Select
                value={appType}
                onValueChange={(value) => {
                  setAppType(value);
                  setOffset(0);
                }}
              >
                <SelectTrigger className="w-[160px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    {t("autotier.decisions.filterAll")}
                  </SelectItem>
                  <SelectItem value="claude">Claude</SelectItem>
                  <SelectItem value="codex">Codex</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label>{t("autotier.decisions.filterCompletion")}</Label>
              <Select
                value={completeFilter}
                onValueChange={(value) => {
                  setCompleteFilter(value);
                  setOffset(0);
                }}
              >
                <SelectTrigger className="w-[160px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    {t("autotier.decisions.filterAll")}
                  </SelectItem>
                  <SelectItem value="complete">
                    {t("autotier.decisions.filterComplete")}
                  </SelectItem>
                  <SelectItem value="incomplete">
                    {t("autotier.decisions.filterIncomplete")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label>{t("autotier.decisions.filterLabel")}</Label>
              <Select
                value={labelFilter}
                onValueChange={(value) => {
                  setLabelFilter(value);
                  setOffset(0);
                }}
              >
                <SelectTrigger className="w-[160px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    {t("autotier.decisions.filterAll")}
                  </SelectItem>
                  <SelectItem value="labeled">
                    {t("autotier.decisions.filterLabeled")}
                  </SelectItem>
                  <SelectItem value="unlabeled">
                    {t("autotier.decisions.filterUnlabeled")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {items.length === 0 ? (
            <p className="text-sm text-muted-foreground py-6 text-center">
              {t("autotier.decisions.emptyList")}
            </p>
          ) : (
            <div className="rounded-md border overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t("autotier.decisions.colTime")}</TableHead>
                    <TableHead>{t("autotier.decisions.colApp")}</TableHead>
                    <TableHead>{t("autotier.decisions.colClient")}</TableHead>
                    <TableHead>{t("autotier.decisions.colCandidate")}</TableHead>
                    <TableHead>{t("autotier.decisions.colActual")}</TableHead>
                    <TableHead>{t("autotier.decisions.colStatus")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {items.map((item) => (
                    <TableRow
                      key={item.decision_id}
                      data-state={
                        selectedId === item.decision_id ? "selected" : undefined
                      }
                      className="cursor-pointer"
                      onClick={() => openDetail(item)}
                    >
                      <TableCell className="whitespace-nowrap text-xs">
                        {formatTimestamp(item.created_at)}
                      </TableCell>
                      <TableCell>{item.app_type}</TableCell>
                      <TableCell className="font-mono text-xs max-w-[140px] truncate">
                        {formatModel(item.client_requested_model, emptyValue)}
                      </TableCell>
                      <TableCell className="text-xs">
                        {item.recommended_slot
                          ? t(
                              `autotier.decisions.slot.${displaySlot(item.recommended_slot)}`,
                              { defaultValue: item.recommended_slot },
                            )
                          : emptyValue}
                      </TableCell>
                      <TableCell className="font-mono text-xs max-w-[140px] truncate">
                        {formatModel(item.actual_outbound_model, emptyValue)}
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-wrap gap-1">
                          {!item.completion.decision_complete && (
                            <Badge variant="secondary">
                              {t("autotier.decisions.incompleteBadge")}
                            </Badge>
                          )}
                          {item.user_label && (
                            <Badge variant="outline">
                              {t(
                                `autotier.decisions.labels.${displayDecisionLabel(item.user_label)}`,
                                { defaultValue: item.user_label },
                              )}
                            </Badge>
                          )}
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}

          <div className="flex items-center justify-between text-sm">
            <span className="text-muted-foreground">
              {t("autotier.decisions.pagination", {
                from: total === 0 ? 0 : offset + 1,
                to: Math.min(offset + PAGE_SIZE, total),
                total,
              })}
            </span>
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={!canPrev}
                onClick={() => setOffset((value) => Math.max(0, value - PAGE_SIZE))}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={!canNext}
                onClick={() => setOffset((value) => value + PAGE_SIZE)}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      {selectedId && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">
              {t("autotier.decisions.detailTitle")}
            </CardTitle>
            <CardDescription className="font-mono text-xs break-all">
              {selectedId}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <Badge variant="secondary" className="whitespace-normal">
              {t("autotier.decisions.shadowNotExecuted")}
            </Badge>

            {detailQuery.isLoading ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                {t("autotier.decisions.detailLoading")}
              </div>
            ) : detail ? (
              <>
                <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                  <RouteGroup
                    title={t("autotier.decisions.groupClient")}
                    model={formatModel(
                      detail.client_requested_model,
                      emptyValue,
                    )}
                    provider={formatProvider(
                      detail.initial_selected_provider,
                      emptyValue,
                    )}
                    empty={emptyValue}
                  />
                  <RouteGroup
                    title={t("autotier.decisions.groupBaseline")}
                    model={formatModel(
                      detail.baseline_outbound_model,
                      emptyValue,
                    )}
                    provider={formatProvider(
                      detail.baseline_outbound_provider,
                      emptyValue,
                    )}
                    empty={emptyValue}
                  />
                  <RouteGroup
                    title={t("autotier.decisions.groupCandidate")}
                    model={formatModel(detail.candidate_model, emptyValue)}
                    provider={formatProvider(
                      detail.candidate_provider,
                      emptyValue,
                    )}
                    empty={emptyValue}
                  />
                  <RouteGroup
                    title={t("autotier.decisions.groupActual")}
                    model={formatModel(
                      detail.actual_outbound_model,
                      emptyValue,
                    )}
                    provider={formatProvider(
                      detail.actual_outbound_provider,
                      emptyValue,
                    )}
                    empty={emptyValue}
                  />
                </div>

                <div className="grid gap-3 md:grid-cols-3 text-sm">
                  <div>
                    <p className="text-xs text-muted-foreground">
                      {t("autotier.decisions.complexity")}
                    </p>
                    <p>{detail.complexity_score.toFixed(2)}</p>
                  </div>
                  <div>
                    <p className="text-xs text-muted-foreground">
                      {t("autotier.decisions.confidence")}
                    </p>
                    <p>{detail.confidence.toFixed(2)}</p>
                  </div>
                  <div>
                    <p className="text-xs text-muted-foreground">
                      {t("autotier.decisions.costRange")}
                    </p>
                    <p className="font-mono text-xs">
                      {formatCostRange(
                        detail.candidate_cost_low_usd,
                        detail.candidate_cost_base_usd,
                        detail.candidate_cost_high_usd,
                        emptyValue,
                      )}
                    </p>
                  </div>
                </div>

                <div className="space-y-2">
                  <p className="text-sm font-medium">
                    {t("autotier.decisions.reasonCodes")}
                  </p>
                  <div className="flex flex-wrap gap-1">
                    {parseJsonStringArray(detail.reason_codes_json).map(
                      (code) => (
                        <Badge key={code} variant="outline">
                          {code}
                        </Badge>
                      ),
                    )}
                    {parseJsonStringArray(detail.reason_codes_json).length ===
                      0 && (
                      <span className="text-sm text-muted-foreground">
                        {emptyValue}
                      </span>
                    )}
                  </div>
                </div>

                <div className="space-y-2">
                  <p className="text-sm font-medium">
                    {t("autotier.decisions.unsafeReasons")}
                  </p>
                  <div className="flex flex-wrap gap-1">
                    {parseJsonStringArray(detail.unsafe_reasons_json).map(
                      (code) => (
                        <Badge key={code} variant="destructive">
                          {code}
                        </Badge>
                      ),
                    )}
                    {parseJsonStringArray(detail.unsafe_reasons_json).length ===
                      0 && (
                      <span className="text-sm text-muted-foreground">
                        {emptyValue}
                      </span>
                    )}
                  </div>
                </div>

                {detail.completion.missing_fields.length > 0 && (
                  <Alert>
                    <AlertTitle>
                      {t("autotier.decisions.incompleteTitle")}
                    </AlertTitle>
                    <AlertDescription>
                      {detail.completion.missing_fields.join(", ")}
                    </AlertDescription>
                  </Alert>
                )}

                <div className="rounded-lg border border-border/60 p-4 space-y-3">
                  <p className="text-sm font-medium">
                    {t("autotier.decisions.labelSection")}
                  </p>
                  <div className="grid gap-3 md:grid-cols-3">
                    <div className="space-y-1">
                      <Label>{t("autotier.decisions.labelField")}</Label>
                      <Select
                        value={draftLabel}
                        onValueChange={(value) =>
                          setDraftLabel(value as AutotierDecisionLabel)
                        }
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {AUTOTIER_DECISION_LABELS.map((label) => (
                            <SelectItem key={label} value={label}>
                              {t(`autotier.decisions.labels.${label}`)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-1">
                      <Label>{t("autotier.decisions.reasonField")}</Label>
                      <Select
                        value={draftReason}
                        onValueChange={(value) =>
                          setDraftReason(
                            value as AutotierDecisionLabelReason | "none",
                          )
                        }
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="none">
                            {t("autotier.decisions.reasonNone")}
                          </SelectItem>
                          {AUTOTIER_DECISION_LABEL_REASONS.map((reason) => (
                            <SelectItem key={reason} value={reason}>
                              {t(`autotier.decisions.labelReasons.${reason}`)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-1 md:col-span-1">
                      <Label>{t("autotier.decisions.noteField")}</Label>
                      <ImeSafeInput
                        value={draftNote}
                        onChange={(event) => setDraftNote(event.target.value)}
                        placeholder={t("autotier.decisions.notePlaceholder")}
                      />
                    </div>
                  </div>
                  <Button
                    size="sm"
                    disabled={upsertLabel.isPending}
                    onClick={() => void saveLabel()}
                  >
                    {upsertLabel.isPending
                      ? t("autotier.decisions.labelSaving")
                      : t("autotier.decisions.labelSave")}
                  </Button>
                </div>
              </>
            ) : (
              <p className="text-sm text-muted-foreground">
                {t("autotier.decisions.detailMissing")}
              </p>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
