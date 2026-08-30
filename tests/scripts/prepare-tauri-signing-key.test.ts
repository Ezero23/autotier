import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SCRIPT = join(process.cwd(), "scripts/prepare-tauri-signing-key.sh");
const WORKFLOW = join(process.cwd(), ".github/workflows/release.yml");

const TWO_LINE = [
  "untrusted comment: test tauri signing key",
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",
].join("\n");

function run(
  envKey: string,
  extraEnv: NodeJS.ProcessEnv = {},
): { status: number; stdout: string; stderr: string; outPath: string; dir: string } {
  const dir = mkdtempSync(join(tmpdir(), "tauri-signing-"));
  const outPath = join(dir, "tauri.key");
  try {
    const stdout = execFileSync("bash", [SCRIPT, outPath], {
      encoding: "utf8",
      env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: envKey, ...extraEnv },
    });
    return { status: 0, stdout, stderr: "", outPath, dir };
  } catch (error) {
    const err = error as { status?: number; stdout?: string; stderr?: string };
    return {
      status: err.status ?? 1,
      stdout: err.stdout ?? "",
      stderr: err.stderr ?? "",
      outPath,
      dir,
    };
  }
}

describe("prepare-tauri-signing-key.sh", () => {
  it("keeps a raw two-line minisign file", () => {
    const result = run(TWO_LINE);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("raw-minisign-file");
    expect(readFileSync(result.outPath, "utf8")).toBe(`${TWO_LINE}\n`);
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("unwraps a base64-encoded two-line file", () => {
    const wrapped = Buffer.from(`${TWO_LINE}\n`, "utf8").toString("base64");
    const result = run(wrapped);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("base64-wrapped-minisign-file");
    expect(readFileSync(result.outPath, "utf8")).toBe(`${TWO_LINE}\n`);
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("wraps a single-line secret with the minisign comment", () => {
    const line = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
    const result = run(line);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("single-line-base64");
    expect(readFileSync(result.outPath, "utf8")).toBe(
      `untrusted comment: tauri signing key\n${line}\n`,
    );
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("rejects an empty secret", () => {
    const result = run("");
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("empty");
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("rejects garbage that is not minisign or base64", () => {
    const result = run("not-a-key!!");
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("格式无法识别");
    rmSync(result.dir, { recursive: true, force: true });
  });
});

describe("release.yml updater signing", () => {
  it("passes Tauri the key file path instead of a second base64 wrap", () => {
    const yml = readFileSync(WORKFLOW, "utf8");
    expect(yml).not.toMatch(/TAURI_SIGNING_PRIVATE_KEY=\$KEY_B64/);
    expect(yml).toMatch(/TAURI_SIGNING_PRIVATE_KEY=\$KEY_PATH/);
    expect(yml).toContain("scripts/prepare-tauri-signing-key.sh");
    expect(yml).toContain("tauri signer sign");
  });

  it("does not require Apple notarization just because updater signing is on", () => {
    const yml = readFileSync(WORKFLOW, "utf8");
    expect(yml).not.toMatch(
      /Notarize macOS DMG[\s\S]*signing_enabled == 'true'/,
    );
    expect(yml).toContain("steps.apple_cert.outputs.apple_signing == 'true'");
  });
});

describe("release.yml does not rewrite a prepared key file", () => {
  it("never base64-encodes the normalized key file back into the env var", () => {
    const yml = readFileSync(WORKFLOW, "utf8");
    expect(yml).not.toContain("KEY_B64=$(base64 <");
  });
});
