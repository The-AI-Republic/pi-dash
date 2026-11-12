// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Enrolment command builder (RUN-008). The form's values become a
// `pidash runner add` invocation in one of three shells. Empty flags are
// omitted entirely; values are trimmed and quoted only when the shell's safe
// character set rejects them. POSIX spreads one flag per continuation line;
// PowerShell and Command Prompt stay on a single line.

export type ShellKind = "posix" | "powershell" | "cmd";

export interface ShellChoice {
  kind: ShellKind;
  label: string;
}

/** Selector order and labels. */
export const SHELL_CHOICES: ShellChoice[] = [
  { kind: "posix", label: "macOS/Linux" },
  { kind: "powershell", label: "PowerShell" },
  { kind: "cmd", label: "Command Prompt" },
];

export interface CommandFields {
  cloudUrl: string;
  workspaceSlug: string;
  projectIdentifier: string;
  podName?: string | undefined;
  runnerName?: string | undefined;
  workingDir?: string | undefined;
  agent: string;
  model?: string | undefined;
  reasoningEffort?: string | undefined;
}

/** One flag and its optional value; `null` is a bare switch. */
type CommandArg = readonly [flag: string, value: string | null];

const POSIX_SAFE = /^[A-Za-z0-9_@%+=:,./-]+$/;
const POWERSHELL_SAFE = /^[A-Za-z0-9_@%+=:,./\\-]+$/;
const CMD_SAFE = /^[A-Za-z0-9_@+=:,./\\-]+$/;

function posixQuote(value: string): string {
  if (POSIX_SAFE.test(value)) return value;
  return `'${value.split("'").join("'\\''")}'`;
}

function powershellQuote(value: string): string {
  if (POWERSHELL_SAFE.test(value)) return value;
  return `'${value.split("'").join("''")}'`;
}

function cmdQuote(value: string): string {
  if (CMD_SAFE.test(value)) return value;
  return `"${value.split('"').join('\\"').split("%").join("^%")}"`;
}

/** Quote a value for the chosen shell. */
export function quoteForShell(shell: ShellKind, value: string): string {
  if (shell === "powershell") return powershellQuote(value);
  if (shell === "cmd") return cmdQuote(value);
  return posixQuote(value);
}

/** The flags a value set contributes, in display order, without empty ones. */
export function commandArgs(fields: CommandFields): CommandArg[] {
  const candidates: Array<readonly [string, string | null | undefined]> = [
    ["--url", fields.cloudUrl],
    ["--workspace", fields.workspaceSlug],
    ["--project", fields.projectIdentifier],
    ["--pod", fields.podName],
    ["--name", fields.runnerName],
    ["--working-dir", fields.workingDir],
    ["--agent", fields.agent],
    ["--model", fields.model],
    ["--reasoning-effort", fields.reasoningEffort],
  ];
  const out: CommandArg[] = [];
  for (const [flag, raw] of candidates) {
    if (raw === null) {
      out.push([flag, null]);
      continue;
    }
    const value = raw?.trim() ?? "";
    if (value !== "") out.push([flag, value]);
  }
  return out;
}

/**
 * Render the full command. POSIX continues each flag on its own line; the
 * other shells join everything with single spaces.
 */
export function buildRunnerCommand(shell: ShellKind, fields: CommandFields): string {
  const command = "pidash runner add";
  const args = commandArgs(fields);
  if (args.length === 0) return command;
  if (shell === "posix") {
    const lines = args.map(([flag, value], index) => {
      const last = index === args.length - 1;
      const body = value === null ? flag : `${flag} ${quoteForShell(shell, value)}`;
      return `  ${body}${last ? "" : " \\"}`;
    });
    return [`${command} \\`, ...lines].join("\n");
  }
  const parts = [command];
  for (const [flag, value] of args) {
    parts.push(flag);
    if (value !== null) parts.push(quoteForShell(shell, value));
  }
  return parts.join(" ");
}

/**
 * The command's cloud URL. A configured base wins; otherwise the serving
 * origin stands in and the caller shows the explanatory note.
 */
export function resolveCommandOrigin(
  configuredBaseUrl: string | null | undefined,
  browserOrigin: string
): { url: string; fellBack: boolean } {
  const configured = configuredBaseUrl?.trim() ?? "";
  if (configured !== "") return { url: configured, fellBack: false };
  return { url: browserOrigin, fellBack: true };
}
