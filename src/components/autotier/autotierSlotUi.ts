import {
  AUTOTIER_REQUIRED_SLOTS,
  AUTOTIER_SLOTS,
  displayCapabilityStatus,
  displaySlot,
  type AutotierProviderSlot,
  type AutotierSlot,
} from "@/lib/api/autotier";

export const AUTOTIER_OPTIONAL_SLOTS = AUTOTIER_SLOTS.filter(
  (slot) => !(AUTOTIER_REQUIRED_SLOTS as readonly string[]).includes(slot),
) as AutotierSlot[];

export type InvalidSlotReason =
  | "empty_model"
  | "unknown_slot"
  | "stale_capability"
  | "failed_capability"
  | "model_missing";

export const INVALID_SLOT_REASON_COPY: Record<InvalidSlotReason, string> = {
  empty_model: "Model ID is required.",
  unknown_slot: "Unknown slot name. This slot is invalid.",
  stale_capability: "Capability is stale. This slot is invalid.",
  failed_capability: "Capability check failed. This slot is invalid.",
  model_missing:
    "This model is no longer on the provider. The slot was not swapped automatically.",
};

export function emptySlotDraft(
  providerId: string,
  slot: string,
): AutotierProviderSlot {
  return {
    provider_id: providerId,
    slot,
    model_id: "",
    capability_status: "unknown",
    supports_tools: null,
    supports_streaming: null,
    supports_vision: null,
    context_limit: null,
    api_format: null,
    pricing_source: null,
    capability_source: null,
    verified_at: null,
    created_at: 0,
    updated_at: 0,
  };
}

/** Always show Cheap/Mid/Strong, even when the provider has no saved slots. */
export function rowsForSlotUi(
  providerId: string,
  slots: AutotierProviderSlot[],
  extraSlots: readonly string[] = [],
): AutotierProviderSlot[] {
  const bySlot = new Map(slots.map((row) => [row.slot, row]));
  const required = AUTOTIER_REQUIRED_SLOTS.map(
    (slot) => bySlot.get(slot) ?? emptySlotDraft(providerId, slot),
  );
  const extras: AutotierProviderSlot[] = [];
  const seen = new Set<string>(AUTOTIER_REQUIRED_SLOTS);
  for (const row of slots) {
    if (seen.has(row.slot)) continue;
    seen.add(row.slot);
    extras.push(row);
  }
  for (const slot of extraSlots) {
    if (seen.has(slot)) continue;
    seen.add(slot);
    extras.push(emptySlotDraft(providerId, slot));
  }
  return [...required, ...extras];
}

export function invalidSlotReasons(input: {
  slot: string;
  model_id: string;
  capability_status: string;
  knownModelIds?: readonly string[] | null;
}): InvalidSlotReason[] {
  const reasons: InvalidSlotReason[] = [];
  const modelId = input.model_id.trim();
  if (!modelId) reasons.push("empty_model");
  if (displaySlot(input.slot) === "unknown") reasons.push("unknown_slot");
  const capability = displayCapabilityStatus(input.capability_status);
  if (capability === "stale") reasons.push("stale_capability");
  if (capability === "failed") reasons.push("failed_capability");
  if (
    modelId &&
    input.knownModelIds &&
    input.knownModelIds.length > 0 &&
    !input.knownModelIds.includes(modelId)
  ) {
    reasons.push("model_missing");
  }
  return reasons;
}

/** Same non-empty model assigned to more than one slot. */
export function duplicateModelGroups(
  slots: readonly { slot: string; model_id: string }[],
): Map<string, string[]> {
  const groups = new Map<string, string[]>();
  for (const row of slots) {
    const modelId = row.model_id.trim();
    if (!modelId) continue;
    const list = groups.get(modelId) ?? [];
    list.push(row.slot);
    groups.set(modelId, list);
  }
  for (const [modelId, list] of [...groups.entries()]) {
    if (list.length < 2) groups.delete(modelId);
  }
  return groups;
}

/**
 * v0.1 never shows a Live-ready badge, even when capability is probed/verified.
 * Capability eligibility is tracked separately for a future Live UI.
 */
export function shouldShowLiveReady(_capabilityStatus?: string): boolean {
  return false;
}

export function isLiveEligibleCapability(status: string): boolean {
  const capability = displayCapabilityStatus(status);
  return capability === "verified" || capability === "probed";
}

export function slotDisplayLabel(slot: string): string {
  switch (displaySlot(slot)) {
    case "cheap":
      return "Cheap";
    case "mid":
      return "Mid";
    case "strong":
      return "Strong";
    case "long_context":
      return "Long Context";
    case "background":
      return "Background";
    default:
      return slot;
  }
}
