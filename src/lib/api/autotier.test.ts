import { describe, expect, it } from "vitest";
import {
  AutotierApiError,
  autotierApi,
  displayAutotierMode,
  displayCapabilityStatus,
  displaySlot,
  effectiveAutotierMode,
  omitSecretFields,
  parseAutotierCommandError,
} from "@/lib/api/autotier";

describe("autotier error enum", () => {
  it("maps backend messages to stable codes", () => {
    expect(parseAutotierCommandError("illegal routing mode: banana")).toBe(
      "illegal_mode",
    );
    expect(
      parseAutotierCommandError("retention_days 11 is not in [7, 14, 30, 90]"),
    ).toBe("illegal_retention");
    expect(parseAutotierCommandError("illegal slot: ultra")).toBe(
      "illegal_slot",
    );
    expect(
      parseAutotierCommandError("illegal capability_status: live-ready"),
    ).toBe("illegal_capability");
    expect(parseAutotierCommandError("provider_id is required")).toBe(
      "missing_provider",
    );
    expect(parseAutotierCommandError("model_id is required")).toBe(
      "missing_model",
    );
    expect(parseAutotierCommandError("db locked")).toBe("unknown");
  });

  it("AutotierApiError carries the code", () => {
    const err = new AutotierApiError("illegal routing mode: full_live");
    expect(err.code).toBe("illegal_mode");
    expect(err.name).toBe("AutotierApiError");
  });
});

describe("unknown enum display", () => {
  it("never treats live or garbage modes as executable", () => {
    expect(displayAutotierMode("shadow")).toBe("shadow");
    expect(displayAutotierMode("off")).toBe("off");
    expect(displayAutotierMode("full_live")).toBe("unknown");
    expect(displayAutotierMode("canary_live")).toBe("unknown");
    expect(displayAutotierMode("banana")).toBe("unknown");
    expect(effectiveAutotierMode("full_live")).toBe("off");
    expect(effectiveAutotierMode("mystery")).toBe("off");
    expect(effectiveAutotierMode("shadow")).toBe("shadow");
  });

  it("unknown capability and slot fall back to unknown", () => {
    expect(displayCapabilityStatus("verified")).toBe("verified");
    expect(displayCapabilityStatus("live-ready")).toBe("unknown");
    expect(displaySlot("cheap")).toBe("cheap");
    expect(displaySlot("ultra")).toBe("unknown");
  });
});

describe("key leak guard", () => {
  it("strips secret-shaped fields from config and slots", () => {
    const leaked = omitSecretFields({
      mode: "shadow",
      api_key: "sk-ant-secret",
      authorization: "Bearer abc",
      slots: [
        {
          provider_id: "p1",
          slot: "cheap",
          token: "leak",
          model_id: "claude-haiku",
        },
      ],
    });
    expect(leaked).toEqual({
      mode: "shadow",
      slots: [
        {
          provider_id: "p1",
          slot: "cheap",
          model_id: "claude-haiku",
        },
      ],
    });
    expect(JSON.stringify(leaked).toLowerCase()).not.toContain("api_key");
    expect(JSON.stringify(leaked)).not.toContain("sk-ant-secret");
  });
});

describe("autotierApi command names", () => {
  it("does not expose live commands", () => {
    const names = Object.keys(autotierApi);
    expect(names.some((n) => /live|canary/i.test(n))).toBe(false);
  });
});
