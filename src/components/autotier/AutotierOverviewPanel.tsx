import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowRight,
  CheckCircle2,
  CircleAlert,
  Eye,
  LockKeyhole,
  Network,
} from "lucide-react";
import {
  useAutotierDecisions,
  useAutotierRoutingConfig,
} from "@/lib/query/autotier";
import type { ProxyTakeoverStatus } from "@/types/proxy";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

interface AutotierOverviewPanelProps {
  isProxyRunning: boolean;
  takeoverStatus?: ProxyTakeoverStatus;
  onOpenDecisions: () => void;
  onOpenSetup: () => void;
}

function formatPercent(value: number, total: number): string {
  if (total <= 0) return "0%";
  return `${Math.round((value / total) * 100)}%`;
}

export function AutotierOverviewPanel({
  isProxyRunning,
  takeoverStatus,
  onOpenDecisions,
  onOpenSetup,
}: AutotierOverviewPanelProps) {
  const { t } = useTranslation();
  const configQuery = useAutotierRoutingConfig();
  const decisionsQuery = useAutotierDecisions({ limit: 100, offset: 0 });

  const metrics = useMemo(() => {
    const items = decisionsQuery.data?.items ?? [];
    const candidateReady = items.filter(
      (item) => item.candidate_model && item.candidate_provider,
    ).length;
    const complete = items.filter((item) => item.is_complete).length;
    const labeled = items.filter((item) => item.user_label).length;
    const slots = items.reduce(
      (counts, item) => {
        if (
          item.recommended_slot === "cheap" ||
          item.recommended_slot === "mid" ||
          item.recommended_slot === "strong"
        ) {
          counts[item.recommended_slot] += 1;
        }
        return counts;
      },
      { cheap: 0, mid: 0, strong: 0 },
    );

    return {
      total: decisionsQuery.data?.total ?? 0,
      sampleSize: items.length,
      candidateReady,
      complete,
      labeled,
      slots,
    };
  }, [decisionsQuery.data]);

  const connectedAgents = Object.values(takeoverStatus ?? {}).filter(
    Boolean,
  ).length;
  const mode = configQuery.data?.mode ?? "shadow";

  const nextAction = useMemo(() => {
    if (!isProxyRunning) {
      return {
        title: t("autotier.console.next.startProxyTitle"),
        description: t("autotier.console.next.startProxyDescription"),
        action: onOpenSetup,
        actionLabel: t("autotier.console.next.openSetup"),
      };
    }
    if (connectedAgents === 0) {
      return {
        title: t("autotier.console.next.connectAgentTitle"),
        description: t("autotier.console.next.connectAgentDescription"),
        action: onOpenSetup,
        actionLabel: t("autotier.console.next.openSetup"),
      };
    }
    if (metrics.total === 0) {
      return {
        title: t("autotier.console.next.sendTrafficTitle"),
        description: t("autotier.console.next.sendTrafficDescription"),
        action: onOpenSetup,
        actionLabel: t("autotier.console.next.checkSetup"),
      };
    }
    if (metrics.candidateReady < metrics.sampleSize) {
      return {
        title: t("autotier.console.next.completeSlotsTitle"),
        description: t("autotier.console.next.completeSlotsDescription"),
        action: onOpenSetup,
        actionLabel: t("autotier.console.next.openSetup"),
      };
    }
    if (metrics.labeled === 0) {
      return {
        title: t("autotier.console.next.reviewTitle"),
        description: t("autotier.console.next.reviewDescription"),
        action: onOpenDecisions,
        actionLabel: t("autotier.console.next.reviewAction"),
      };
    }
    return {
      title: t("autotier.console.next.keepObservingTitle"),
      description: t("autotier.console.next.keepObservingDescription"),
      action: onOpenDecisions,
      actionLabel: t("autotier.console.next.reviewAction"),
    };
  }, [
    connectedAgents,
    isProxyRunning,
    metrics.candidateReady,
    metrics.labeled,
    metrics.sampleSize,
    metrics.total,
    onOpenDecisions,
    onOpenSetup,
    t,
  ]);

  return (
    <div className="space-y-4">
      <div className="rounded-2xl border border-indigo-500/20 bg-gradient-to-br from-indigo-500/10 via-background to-violet-500/5 p-6">
        <div className="flex flex-col gap-5 lg:flex-row lg:items-center lg:justify-between">
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-xl font-semibold">
                {t("autotier.console.title")}
              </h2>
              <Badge variant="secondary">Shadow</Badge>
              <Badge variant={isProxyRunning ? "default" : "outline"}>
                {isProxyRunning
                  ? t("autotier.console.observing")
                  : t("autotier.console.waiting")}
              </Badge>
            </div>
            <p className="max-w-3xl text-sm text-muted-foreground">
              {t("autotier.console.description")}
            </p>
          </div>
          <Button onClick={nextAction.action} className="shrink-0 gap-2">
            {nextAction.actionLabel}
            <ArrowRight className="h-4 w-4" />
          </Button>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <Network className="h-5 w-5 text-emerald-500" />
              <Badge variant={isProxyRunning ? "default" : "outline"}>
                {isProxyRunning
                  ? t("autotier.console.status.connected")
                  : t("autotier.console.status.disconnected")}
              </Badge>
            </div>
            <CardTitle className="text-base">
              {t("autotier.console.status.trafficTitle")}
            </CardTitle>
            <CardDescription>
              {t("autotier.console.status.agentCount", {
                count: connectedAgents,
              })}
            </CardDescription>
          </CardHeader>
        </Card>

        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <Eye className="h-5 w-5 text-indigo-500" />
              <span className="text-2xl font-semibold tabular-nums">
                {metrics.total}
              </span>
            </div>
            <CardTitle className="text-base">
              {t("autotier.console.status.decisionsTitle")}
            </CardTitle>
            <CardDescription>
              {t("autotier.console.status.candidateCoverage", {
                percent: formatPercent(
                  metrics.candidateReady,
                  metrics.sampleSize,
                ),
              })}
            </CardDescription>
          </CardHeader>
        </Card>

        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <LockKeyhole className="h-5 w-5 text-violet-500" />
              <Badge variant="secondary">{mode}</Badge>
            </div>
            <CardTitle className="text-base">
              {t("autotier.console.status.safetyTitle")}
            </CardTitle>
            <CardDescription>
              {t("autotier.console.status.safetyDescription")}
            </CardDescription>
          </CardHeader>
        </Card>
      </div>

      <div className="grid gap-4 lg:grid-cols-[1.35fr_1fr]">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">
              {t("autotier.console.distribution.title")}
            </CardTitle>
            <CardDescription>
              {t("autotier.console.distribution.description", {
                count: metrics.sampleSize,
              })}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {(["cheap", "mid", "strong"] as const).map((slot) => (
              <div key={slot} className="space-y-1.5">
                <div className="flex items-center justify-between text-sm">
                  <span>{t(`autotier.console.distribution.${slot}`)}</span>
                  <span className="font-medium tabular-nums">
                    {metrics.slots[slot]}
                  </span>
                </div>
                <div className="h-2 overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-full rounded-full bg-indigo-500 transition-all"
                    style={{
                      width: formatPercent(
                        metrics.slots[slot],
                        metrics.sampleSize,
                      ),
                    }}
                  />
                </div>
              </div>
            ))}
            <div className="flex flex-wrap gap-2 pt-1 text-xs text-muted-foreground">
              <span>
                {t("autotier.console.distribution.complete", {
                  count: metrics.complete,
                })}
              </span>
              <span>·</span>
              <span>
                {t("autotier.console.distribution.labeled", {
                  count: metrics.labeled,
                })}
              </span>
            </div>
          </CardContent>
        </Card>

        <Card className="border-indigo-500/20">
          <CardHeader>
            <div className="flex items-center gap-2">
              {metrics.total > 0 ? (
                <CheckCircle2 className="h-5 w-5 text-emerald-500" />
              ) : (
                <CircleAlert className="h-5 w-5 text-amber-500" />
              )}
              <CardTitle className="text-base">
                {t("autotier.console.next.title")}
              </CardTitle>
            </div>
            <CardDescription>{nextAction.title}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <p className="text-sm text-muted-foreground">
              {nextAction.description}
            </p>
            <Button
              variant="outline"
              onClick={nextAction.action}
              className="w-full gap-2"
            >
              {nextAction.actionLabel}
              <ArrowRight className="h-4 w-4" />
            </Button>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
