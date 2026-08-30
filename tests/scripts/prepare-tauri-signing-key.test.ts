import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SCRIPT = join(process.cwd(), "scripts/prepare-tauri-signing-key.sh");
const WORKFLOW = join(process.cwd(), ".github/workflows/release.yml");

const TWO_LINE = [
  "untrusted comment: test tauri signing key",
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",
].join("\n");

const GENERATE_FORMAT = Buffer.from(`${TWO_LINE}\n`, "utf8").toString("base64");

function run(envKey: string): {
  status: number;
  stdout: string;
  stderr: string;
  outPath: string;
  dir: string;
} {
  const dir = mkdtempSync(join(tmpdir(), "tauri-signing-"));
  const outPath = join(dir, "tauri.key");
  try {
    const stdout = execFileSync("bash", [SCRIPT, outPath], {
      encoding: "utf8",
      env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: envKey },
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
  it("encodes a raw two-line minisign file into generate format", () => {
    const result = run(`${TWO_LINE}\n`);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("raw-minisign-to-generate-format");
    const official = readFileSync(result.outPath, "utf8");
    expect(official).toBe(GENERATE_FORMAT);
    expect(official).not.toContain("\n");
    expect(official).not.toContain("untrusted comment:");
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("keeps an official generate-format secret as a single line", () => {
    const result = run(GENERATE_FORMAT);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("generate-format");
    expect(readFileSync(result.outPath, "utf8")).toBe(GENERATE_FORMAT);
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("wraps a bare secret line, then encodes generate format", () => {
    const line = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
    const result = run(line);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("single-line-secret-to-generate-format");
    const decoded = Buffer.from(readFileSync(result.outPath, "utf8"), "base64").toString("utf8");
    expect(decoded).toBe(`untrusted comment: tauri signing key\n${line}\n`);
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("rejects an empty secret", () => {
    const result = run("");
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("empty");
    rmSync(result.dir, { recursive: true, force: true });
  });

  it("rejects a generate-format public key", () => {
    const pub = [
      "untrusted comment: minisign public key: DEADBEEF",
      "RWTBxYnSH0TY3ebOR/MjU9E6vrKw6arM8G7cIEqwi0MkNtH5DFrgPEaF",
    ].join("\n");
    const result = run(Buffer.from(`${pub}\n`, "utf8").toString("base64"));
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("PUBLIC key");
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
  it("passes Tauri a generate-format key file path, not a second ad-hoc wrap", () => {
    const yml = readFileSync(WORKFLOW, "utf8");
    expect(yml).not.toMatch(/TAURI_SIGNING_PRIVATE_KEY=\$KEY_B64/);
    expect(yml).toMatch(/TAURI_SIGNING_PRIVATE_KEY=\$KEY_PATH/);
    expect(yml).toContain("scripts/prepare-tauri-signing-key.sh");
    expect(yml).toContain("tauri signer sign");
    expect(yml).toContain("--private-key-path");
  });

  it("does not require Apple notarization just because updater signing is on", () => {
    const yml = readFileSync(WORKFLOW, "utf8");
    expect(yml).toContain("steps.apple_cert.outputs.apple_signing == 'true'");
    expect(yml).not.toContain(
      "if: runner.os == 'macOS' && steps.tauri_signing.outputs.signing_enabled == 'true'\n        shell: bash\n        timeout-minutes: 30",
    );
  });

  it("never base64-encodes the normalized key file back into the env var", () => {
    const yml = readFileSync(WORKFLOW, "utf8");
    expect(yml).not.toContain("KEY_B64=$(base64 <");
  });
});

describe("official generate format can be signed by tauri signer", () => {
  it("signs a smoke file with a freshly generated key after normalization", () => {
    const dir = mkdtempSync(join(tmpdir(), "tauri-sign-"));
    const generated = join(dir, "generated.key");
    const smoke = join(dir, "smoke.txt");
    let prepared: ReturnType<typeof run> | undefined;
    try {
      execFileSync(
        "pnpm",
        ["exec", "tauri", "signer", "generate", "-w", generated, "--ci", "-p", ""],
        { encoding: "utf8", cwd: process.cwd() },
      );
      const generatedBody = readFileSync(generated, "utf8").trim();
      prepared = run(generatedBody);
      expect(prepared.status).toBe(0);
      expect(readFileSync(prepared.outPath, "utf8")).toBe(generatedBody);
      writeFileSync(smoke, "autotier updater key smoke\n");
      execFileSync(
        "pnpm",
        [
          "exec",
          "tauri",
          "signer",
          "sign",
          "--private-key-path",
          prepared.outPath,
          "--password",
          "",
          smoke,
        ],
        { encoding: "utf8", cwd: process.cwd() },
      );
      expect(readFileSync(`${smoke}.sig`, "utf8").length).toBeGreaterThan(20);
      // Two-line decoded files are not valid for --private-key-path.
      const twoLinePath = join(dir, "two-line.key");
      writeFileSync(twoLinePath, Buffer.from(generatedBody, "base64"));
      expect(() =>
        execFileSync(
          "pnpm",
          ["exec", "tauri", "signer", "sign", "--private-key-path", twoLinePath, "--password", "", smoke],
          { encoding: "utf8", cwd: process.cwd() },
        ),
      ).toThrow();
    } finally {
      rmSync(dir, { recursive: true, force: true });
      if (typeof prepared !== "undefined") {
        rmSync(prepared.dir, { recursive: true, force: true });
      }
    }
  });
});
