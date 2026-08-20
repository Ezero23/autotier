import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  autotierApi,
  type AutotierDecisionQueryFilter,
  type AutotierProviderSlot,
  type AutotierRetentionDays,
  type AutotierSaveConfigInput,
  type UpsertDecisionLabelInput,
} from "@/lib/api/autotier";

export const autotierKeys = {
  all: ["autotier"] as const,
  config: ["autotier", "config"] as const,
  slots: (providerId: string) => ["autotier", "slots", providerId] as const,
  required: (providerId: string) =>
    ["autotier", "required", providerId] as const,
  decisions: (filter: AutotierDecisionQueryFilter) =>
    ["autotier", "decisions", filter] as const,
  decisionDetail: (decisionId: string) =>
    ["autotier", "decision", decisionId] as const,
};

export function useAutotierRoutingConfig(enabled = true) {
  return useQuery({
    queryKey: autotierKeys.config,
    queryFn: () => autotierApi.getRoutingConfig(),
    enabled,
  });
}

export function useSaveAutotierRoutingConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: AutotierSaveConfigInput) =>
      autotierApi.saveRoutingConfig(input),
    onSuccess: (config) => {
      queryClient.setQueryData(autotierKeys.config, config);
    },
  });
}

export function useAutotierProviderSlots(providerId: string, enabled = true) {
  return useQuery({
    queryKey: autotierKeys.slots(providerId),
    queryFn: () => autotierApi.listProviderSlots(providerId),
    enabled: enabled && providerId.length > 0,
  });
}

export function useUpsertAutotierProviderSlot() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (slot: AutotierProviderSlot) =>
      autotierApi.upsertProviderSlot(slot),
    onSuccess: (slot) => {
      queryClient.invalidateQueries({
        queryKey: autotierKeys.slots(slot.provider_id),
      });
      queryClient.invalidateQueries({
        queryKey: autotierKeys.required(slot.provider_id),
      });
    },
  });
}

export function useDeleteAutotierProviderSlot() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { providerId: string; slot: string }) =>
      autotierApi.deleteProviderSlot(args.providerId, args.slot),
    onSuccess: (_n, args) => {
      queryClient.invalidateQueries({
        queryKey: autotierKeys.slots(args.providerId),
      });
      queryClient.invalidateQueries({
        queryKey: autotierKeys.required(args.providerId),
      });
    },
  });
}

export function useAutotierRequiredSlots(providerId: string, enabled = true) {
  return useQuery({
    queryKey: autotierKeys.required(providerId),
    queryFn: () => autotierApi.requiredSlotsStatus(providerId),
    enabled: enabled && providerId.length > 0,
  });
}

export function useClearAutotierDecisions() {
  return useMutation({
    mutationFn: () => autotierApi.clearDecisions(),
  });
}

export function usePruneAutotierDecisions() {
  return useMutation({
    mutationFn: (retentionDays?: AutotierRetentionDays) =>
      autotierApi.pruneDecisions(retentionDays),
  });
}

export function useAutotierDecisions(
  filter: AutotierDecisionQueryFilter,
  enabled = true,
) {
  return useQuery({
    queryKey: autotierKeys.decisions(filter),
    queryFn: () => autotierApi.queryDecisions(filter),
    enabled,
  });
}

export function useAutotierDecisionDetail(decisionId: string | null) {
  return useQuery({
    queryKey: autotierKeys.decisionDetail(decisionId ?? ""),
    queryFn: () =>
      decisionId
        ? autotierApi.getDecisionDetail(decisionId)
        : Promise.resolve(null),
    enabled: Boolean(decisionId),
  });
}

export function useUpsertAutotierDecisionLabel() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: UpsertDecisionLabelInput) =>
      autotierApi.upsertDecisionLabel(input),
    onSuccess: (label) => {
      queryClient.invalidateQueries({ queryKey: autotierKeys.all });
      queryClient.invalidateQueries({
        queryKey: autotierKeys.decisionDetail(label.decision_id),
      });
    },
  });
}
