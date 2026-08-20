import { invoke } from "@tauri-apps/api/core";
import { extractErrorMessage } from "@/utils/errorUtils";

/** v0.1 激活的路由模式。其它值不得当作可执行 Live。 */
export const AUTOTIER_MODES_V01 = ["off", "shadow"] as const;
export type AutotierModeV01 = (typeof AUTOTIER_MODES_V01)[number];

export const AUTOTIER_SLOTS = [
  "cheap",
  "mid",
  "strong",
  "long_context",
  "background",
] as const;
export type AutotierSlot = (typeof AUTOTIER_SLOTS)[number];
export const AUTOTIER_REQUIRED_SLOTS = ["cheap", "mid", "strong"] as const;

export const AUTOTIER_CAPABILITY_STATUSES = [
  "unknown",
  "declared",
  "probed",
  "verified",
  "stale",
  "failed",
] as const;
export type AutotierCapabilityStatus =
  (typeof AUTOTIER_CAPABILITY_STATUSES)[number];

export const AUTOTIER_RETENTION_DAYS = [7, 14, 30, 90] as const;
export type AutotierRetentionDays = (typeof AUTOTIER_RETENTION_DAYS)[number];

export const AUTOTIER_DECISION_LABELS = [
  "correct",
  "should_be_stronger",
  "could_be_cheaper",
  "unsure",
] as const;
export type AutotierDecisionLabel = (typeof AUTOTIER_DECISION_LABELS)[number];

export const AUTOTIER_DECISION_LABEL_REASONS = [
  "tool_failure_risk",
  "long_context",
  "architecture_reasoning",
  "simple_formatting",
  "background_task",
  "wrong_provider_capability",
  "cache_risk",
  "other",
] as const;
export type AutotierDecisionLabelReason =
  (typeof AUTOTIER_DECISION_LABEL_REASONS)[number];

export interface AutotierDecisionCompletionStatus {
  decision_complete: boolean;
  usage_linked: boolean;
  missing_fields: string[];
}

export interface AutotierDecisionListItem {
  decision_id: string;
  created_at: number;
  completed_at: number | null;
  app_type: string;
  session_id_hash: string;
  mode: string;
  client_requested_model: string;
  initial_selected_provider: string | null;
  baseline_outbound_model: string | null;
  baseline_outbound_provider: string | null;
  recommended_slot: string | null;
  candidate_model: string | null;
  candidate_provider: string | null;
  actual_outbound_model: string | null;
  actual_outbound_provider: string | null;
  complexity_score: number;
  confidence: number;
  safe_to_execute: boolean;
  is_complete: boolean;
  error_code: string | null;
  user_label: string | null;
  completion: AutotierDecisionCompletionStatus;
}

export interface AutotierDecisionListPage {
  items: AutotierDecisionListItem[];
  total: number;
  limit: number;
  offset: number;
}

export interface AutotierDecisionLabelRecord {
  decision_id: string;
  label: string;
  reason: string | null;
  note: string | null;
  created_at: number;
  updated_at: number;
}

export interface AutotierDecisionDetail
  extends Omit<AutotierDecisionListItem, "user_label"> {
  autotier_mutated_request: boolean;
  upstream_message_id: string | null;
  usage_request_id: string | null;
  reason_codes_json: string;
  unsafe_reasons_json: string;
  feature_json: string;
  feature_version: string;
  classifier_version: string;
  policy_version: string;
  actual_input_tokens: number | null;
  actual_output_tokens: number | null;
  actual_cache_read_tokens: number | null;
  actual_cache_write_5m_tokens: number | null;
  actual_cache_write_1h_tokens: number | null;
  actual_cost_usd: string | null;
  candidate_cost_low_usd: string | null;
  candidate_cost_base_usd: string | null;
  candidate_cost_high_usd: string | null;
  cost_assumptions_json: string;
  status_code: number | null;
  outcome: string | null;
  retry_count: number;
  fallback_count: number;
  user_label: AutotierDecisionLabelRecord | null;
}

export interface AutotierDecisionQueryFilter {
  since_ms?: number;
  until_ms?: number;
  session_id_hash?: string;
  app_type?: string;
  client_requested_model?: string;
  recommended_slot?: string;
  candidate_model?: string;
  actual_outbound_model?: string;
  provider?: string;
  reason_code?: string;
  unsafe_reason?: string;
  confidence_min?: number;
  confidence_max?: number;
  cache_protected?: boolean;
  is_complete?: boolean;
  label?: string;
  has_label?: boolean;
  limit?: number;
  offset?: number;
}

export interface UpsertDecisionLabelInput {
  decision_id: string;
  label: AutotierDecisionLabel;
  reason?: AutotierDecisionLabelReason | null;
  note?: string | null;
}

export interface AutotierExportManifest {
  export_schema_version: number;
  generated_at: string;
  decision_count: number;
  label_count: number;
  contains_raw_prompt: boolean;
  contains_credentials: boolean;
}

export interface AutotierExportResult {
  output_dir: string;
  manifest: AutotierExportManifest;
}

export interface AutotierReplayReport {
  replayed: number;
  matched: number;
  mismatches: Array<{
    decision_id: string;
    field: string;
    expected: string;
    actual: string;
  }>;
  malformed_rows: Array<{ line_number: number; error: string }>;
}

export interface AutotierEvalMetrics {
  tune_count: number;
  holdout_count: number;
  strong_recall: number;
  unsafe_downgrade: number;
  cache_adjusted_saving_usd: number;
  holdout_sample_sufficient: boolean;
  warnings: string[];
}

export interface AutotierEvalReport {
  sessions: number;
  metrics: AutotierEvalMetrics;
}

export interface AutotierLegacyDataStatus {
  legacy_dir: string;
  legacy_db_exists: boolean;
  autotier_dir: string;
  autotier_db_exists: boolean;
}

export interface AutotierImportLegacyResult {
  imported_from: string;
  imported_to: string;
  backup_path: string | null;
}

export type AutotierCommandErrorCode =
  | "illegal_mode"
  | "illegal_retention"
  | "illegal_slot"
  | "illegal_capability"
  | "missing_provider"
  | "missing_model"
  | "unknown";

const SECRET_FIELD = /^(api[_-]?key|authorization|secret|token|password)$/i;

export interface AutotierRoutingConfig {
  mode: string;
  retention_days: number;
  raw_prompt_opt_in: boolean;
  classifier_version: string;
  feature_version: string;
  policy_version: string;
  capability_table_version: string;
  cost_model_version: string;
  cache_stats_version: string;
  updated_at: number;
  degraded_from: string | null;
}

export interface AutotierSaveConfigInput {
  mode: AutotierModeV01;
  retention_days: AutotierRetentionDays;
}

export interface AutotierProviderSlot {
  provider_id: string;
  slot: string;
  model_id: string;
  capability_status: string;
  supports_tools: boolean | null;
  supports_streaming: boolean | null;
  supports_vision: boolean | null;
  context_limit: number | null;
  api_format: string | null;
  pricing_source: string | null;
  capability_source: string | null;
  verified_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface AutotierProviderModelPricing {
  provider_id: string;
  model_id: string;
  display_name: string;
  input_cost_per_million: string;
  output_cost_per_million: string;
  cache_read_cost_per_million: string;
  cache_creation_cost_per_million: string;
  price_source: string;
  observed_at: number;
}

export type AutotierProviderModelPricingInput = Omit<
  AutotierProviderModelPricing,
  "observed_at"
> & {
  price_source?: string | null;
};

export interface AutotierRequiredSlotsStatus {
  provider_id: string;
  complete: boolean;
  present: string[];
  missing: string[];
}

export class AutotierApiError extends Error {
  readonly code: AutotierCommandErrorCode;

  constructor(message: string) {
    super(message);
    this.name = "AutotierApiError";
    this.code = parseAutotierCommandError(message);
  }
}

export function parseAutotierCommandError(
  message: string,
): AutotierCommandErrorCode {
  const msg = message.toLowerCase();
  if (msg.includes("illegal routing mode")) return "illegal_mode";
  if (msg.includes("retention_days")) return "illegal_retention";
  if (msg.includes("illegal slot")) return "illegal_slot";
  if (msg.includes("illegal capability_status")) return "illegal_capability";
  if (msg.includes("provider_id is required")) return "missing_provider";
  if (msg.includes("model_id is required")) return "missing_model";
  return "unknown";
}

/** 未知 / Live 模式安全显示：永不展示为可执行 Live。 */
export function displayAutotierMode(mode: string): AutotierModeV01 | "unknown" {
  if (mode === "off" || mode === "shadow") return mode;
  return "unknown";
}

/** 未知 Mode 按方案视为 Off。 */
export function effectiveAutotierMode(mode: string): AutotierModeV01 {
  return mode === "shadow" ? "shadow" : "off";
}

export function displayCapabilityStatus(
  status: string,
): AutotierCapabilityStatus {
  return (AUTOTIER_CAPABILITY_STATUSES as readonly string[]).includes(status)
    ? (status as AutotierCapabilityStatus)
    : "unknown";
}

export function displaySlot(slot: string): AutotierSlot | "unknown" {
  return (AUTOTIER_SLOTS as readonly string[]).includes(slot)
    ? (slot as AutotierSlot)
    : "unknown";
}

export function displayDecisionLabel(
  label: string,
): AutotierDecisionLabel | "unknown" {
  return (AUTOTIER_DECISION_LABELS as readonly string[]).includes(label)
    ? (label as AutotierDecisionLabel)
    : "unknown";
}

export function displayDecisionLabelReason(
  reason: string,
): AutotierDecisionLabelReason | "unknown" {
  return (AUTOTIER_DECISION_LABEL_REASONS as readonly string[]).includes(reason)
    ? (reason as AutotierDecisionLabelReason)
    : "unknown";
}

export function parseJsonStringArray(raw: string): string[] {
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((item): item is string => typeof item === "string");
  } catch {
    return [];
  }
}

export function omitSecretFields<T>(value: T): T {
  if (Array.isArray(value)) {
    return value.map((item) => omitSecretFields(item)) as T;
  }
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [key, nested] of Object.entries(
      value as Record<string, unknown>,
    )) {
      if (SECRET_FIELD.test(key)) continue;
      out[key] = omitSecretFields(nested);
    }
    return out as T;
  }
  return value;
}

async function invokeAutotier<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    const raw = await invoke<T>(command, args);
    return omitSecretFields(raw);
  } catch (error) {
    throw new AutotierApiError(extractErrorMessage(error) || String(error));
  }
}

export const autotierApi = {
  getRoutingConfig(): Promise<AutotierRoutingConfig> {
    return invokeAutotier("autotier_get_routing_config");
  },

  saveRoutingConfig(
    input: AutotierSaveConfigInput,
  ): Promise<AutotierRoutingConfig> {
    return invokeAutotier("autotier_save_routing_config", { input });
  },

  listProviderSlots(providerId: string): Promise<AutotierProviderSlot[]> {
    return invokeAutotier("autotier_list_provider_slots", { providerId });
  },
  listProviderModelPricing(
    providerId: string,
  ): Promise<AutotierProviderModelPricing[]> {
    return invokeAutotier("autotier_list_provider_model_pricing", {
      providerId,
    });
  },
  upsertProviderModelPricing(
    input: AutotierProviderModelPricingInput,
  ): Promise<AutotierProviderModelPricing> {
    return invokeAutotier("autotier_upsert_provider_model_pricing", { input });
  },
  deleteProviderModelPricing(
    providerId: string,
    modelId: string,
  ): Promise<number> {
    return invokeAutotier("autotier_delete_provider_model_pricing", {
      providerId,
      modelId,
    });
  },
  upsertProviderSlot(
    slot: AutotierProviderSlot,
  ): Promise<AutotierProviderSlot> {
    return invokeAutotier("autotier_upsert_provider_slot", { slot });
  },

  deleteProviderSlot(providerId: string, slot: string): Promise<number> {
    return invokeAutotier("autotier_delete_provider_slot", {
      providerId,
      slot,
    });
  },

  requiredSlotsStatus(
    providerId: string,
  ): Promise<AutotierRequiredSlotsStatus> {
    return invokeAutotier("autotier_required_slots_status", { providerId });
  },

  clearDecisions(): Promise<void> {
    return invokeAutotier("autotier_clear_decisions");
  },

  pruneDecisions(retentionDays?: AutotierRetentionDays): Promise<number> {
    return invokeAutotier("autotier_prune_decisions", { retentionDays });
  },

  queryDecisions(
    filter: AutotierDecisionQueryFilter,
  ): Promise<AutotierDecisionListPage> {
    return invokeAutotier("autotier_query_decisions", { filter });
  },

  getDecisionDetail(
    decisionId: string,
  ): Promise<AutotierDecisionDetail | null> {
    return invokeAutotier("autotier_get_decision_detail", { decisionId });
  },

  upsertDecisionLabel(
    input: UpsertDecisionLabelInput,
  ): Promise<AutotierDecisionLabelRecord> {
    return invokeAutotier("autotier_upsert_decision_label", { input });
  },

  getDecisionLabel(
    decisionId: string,
  ): Promise<AutotierDecisionLabelRecord | null> {
    return invokeAutotier("autotier_get_decision_label", { decisionId });
  },

  exportDecisions(outputDir: string): Promise<AutotierExportResult> {
    return invokeAutotier("autotier_export_decisions", { outputDir });
  },

  replayExport(exportDir: string): Promise<AutotierReplayReport> {
    return invokeAutotier("autotier_replay_export", { exportDir });
  },

  evaluateExport(
    exportDir: string,
    splitSeed?: number,
  ): Promise<AutotierEvalReport> {
    return invokeAutotier("autotier_evaluate_export", {
      exportDir,
      splitSeed: splitSeed ?? null,
    });
  },

  detectLegacyData(): Promise<AutotierLegacyDataStatus> {
    return invokeAutotier("autotier_detect_legacy_data");
  },

  importLegacyData(): Promise<AutotierImportLegacyResult> {
    return invokeAutotier("autotier_import_legacy_data");
  },
};
