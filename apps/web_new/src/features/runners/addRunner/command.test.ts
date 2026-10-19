// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Enrolment command (RUN-008): per-shell quoting, multi-line POSIX, omitted
// empty flags and the origin fallback.
import { describe, expect, it } from "vitest";

import { buildRunnerCommand, commandArgs, quoteForShell, resolveCommandOrigin, SHELL_CHOICES } from "./command.js";

const FIELDS = {
  cloudUrl: "https://cloud.test",
  workspaceSlug: "acme",
  projectIdentifier: "ACME01",
  podName: "acme-pod",
  runnerName: "laptop_1",
  workingDir: "/tmp/parity work",
  agent: "claude-code",
  model: "claude-opus-4-8",
  reasoningEffort: undefined,
};

describe("shell selector", () => {
  it("offers the three shells in order", () => {
    expect(SHELL_CHOICES).toEqual([
      { kind: "posix", label: "macOS/Linux" },
      { kind: "powershell", label: "PowerShell" },
      { kind: "cmd", label: "Command Prompt" },
    ]);
  });
});

describe("quoting", () => {
  it("leaves safe tokens bare", () => {
    expect(quoteForShell("posix", "a_b-1.2")).toBe("a_b-1.2");
    expect(quoteForShell("powershell", "C:\\work")).toBe("C:\\work");
    expect(quoteForShell("cmd", "C:\\work")).toBe("C:\\work");
  });

  it("quotes POSIX with single-quote escaping", () => {
    expect(quoteForShell("posix", "it's here")).toBe("'it'\\''s here'");
    expect(quoteForShell("posix", "two words")).toBe("'two words'");
  });

  it("doubles single quotes for PowerShell", () => {
    expect(quoteForShell("powershell", "it's here")).toBe("'it''s here'");
  });

  it("escapes double quotes and percent signs for Command Prompt", () => {
    expect(quoteForShell("cmd", 'say "hi"')).toBe('"say \\"hi\\""');
    expect(quoteForShell("cmd", "100%")).toBe('"100^%"');
  });
});

describe("commandArgs", () => {
  it("drops empty and whitespace-only values", () => {
    expect(commandArgs({ ...FIELDS, podName: undefined, runnerName: "  ", workingDir: "" })).toEqual([
      ["--url", "https://cloud.test"],
      ["--workspace", "acme"],
      ["--project", "ACME01"],
      ["--agent", "claude-code"],
      ["--model", "claude-opus-4-8"],
    ]);
  });

  it("trims values that survive", () => {
    expect(commandArgs({ ...FIELDS, runnerName: " padded " })).toContainEqual(["--name", "padded"]);
  });

  it("keeps bare switches supplied as null", () => {
    expect(commandArgs({ ...FIELDS, reasoningEffort: null as unknown as undefined })).toContainEqual([
      "--reasoning-effort",
      null,
    ]);
  });
});

describe("buildRunnerCommand", () => {
  it("renders POSIX multi-line with trailing continuations", () => {
    const command = buildRunnerCommand("posix", FIELDS);
    const lines = command.split("\n");
    expect(lines[0]).toBe("pidash runner add \\");
    expect(lines.at(-1)).not.toContain("\\");
    expect(command).toContain("  --project ACME01 \\");
    expect(command).toContain("--working-dir '/tmp/parity work'");
    expect(command).not.toContain("--reasoning-effort");
  });

  it("renders a bare switch without a value", () => {
    const command = buildRunnerCommand("posix", {
      ...FIELDS,
      podName: null as unknown as undefined,
    });
    expect(command.split("\n")).toContain("  --pod \\");
  });

  it("renders PowerShell and Command Prompt single-line", () => {
    for (const shell of ["powershell", "cmd"] as const) {
      const command = buildRunnerCommand(shell, FIELDS);
      expect(command).not.toContain("\n");
      expect(command.startsWith("pidash runner add --url ")).toBe(true);
    }
    expect(buildRunnerCommand("cmd", FIELDS)).toContain('--working-dir "/tmp/parity work"');
  });

  it("returns the bare command when every field is empty", () => {
    const empty = {
      cloudUrl: "",
      workspaceSlug: "",
      projectIdentifier: "",
      agent: "",
    };
    expect(buildRunnerCommand("posix", empty)).toBe("pidash runner add");
    expect(buildRunnerCommand("cmd", empty)).toBe("pidash runner add");
  });

  it("uses the last POSIX flag without a continuation", () => {
    const command = buildRunnerCommand("posix", { ...FIELDS, agent: "", model: "", workingDir: "" });
    const lines = command.split("\n");
    expect(lines.at(-1)).toBe("  --name laptop_1");
  });
});

describe("resolveCommandOrigin", () => {
  it("prefers a configured base", () => {
    expect(resolveCommandOrigin("https://api.test", "https://app.test")).toEqual({
      url: "https://api.test",
      fellBack: false,
    });
  });

  it("falls back to the browser origin when the base is blank or absent", () => {
    expect(resolveCommandOrigin("", "https://app.test")).toEqual({ url: "https://app.test", fellBack: true });
    expect(resolveCommandOrigin(undefined, "https://app.test")).toEqual({
      url: "https://app.test",
      fellBack: true,
    });
  });
});
