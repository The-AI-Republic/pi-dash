/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";

type Props = {
  cloudUrl: string;
  workspaceSlug: string;
  projectIdentifier: string;
  podName?: string;
  name?: string;
  workingDir?: string;
  agent?: string;
  isUsingBrowserOrigin?: boolean;
};

type TShell = "posix" | "powershell" | "cmd";

const SHELL_OPTIONS: TShell[] = ["posix", "powershell", "cmd"];

function posixArg(value: string): string {
  const trimmed = value.trim();
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(trimmed)) return trimmed;
  return `'${trimmed.replaceAll("'", "'\\''")}'`;
}

function powershellArg(value: string): string {
  const trimmed = value.trim();
  if (/^[A-Za-z0-9_@%+=:,./\\-]+$/.test(trimmed)) return trimmed;
  return `'${trimmed.replaceAll("'", "''")}'`;
}

function cmdArg(value: string): string {
  const trimmed = value.trim();
  if (/^[A-Za-z0-9_@+=:,./\\-]+$/.test(trimmed)) return trimmed;
  return `"${trimmed.replaceAll('"', '\\"').replaceAll("%", "^%")}"`;
}

function shellArg(shell: TShell, value: string): string {
  if (shell === "powershell") return powershellArg(value);
  if (shell === "cmd") return cmdArg(value);
  return posixArg(value);
}

function usableArgs(args: Array<[string, string | null | undefined]>) {
  return args
    .map(([flag, value]) => [flag, value === null ? null : value?.trim()] as const)
    .filter((arg): arg is readonly [string, string | null] => arg[1] === null || Boolean(arg[1]));
}

function posixCommandLines(command: string, args: Array<[string, string | null | undefined]>): string[] {
  const renderedArgs = usableArgs(args);

  if (renderedArgs.length === 0) return [command];

  return [
    `${command} \\`,
    ...renderedArgs.map(([flag, value], index) => {
      const suffix = index === renderedArgs.length - 1 ? "" : " \\";
      if (value === null) return `  ${flag}${suffix}`;
      return `  ${flag} ${shellArg("posix", value)}${suffix}`;
    }),
  ];
}

function oneLineCommand(command: string, shell: TShell, args: Array<[string, string | null | undefined]>): string {
  const parts = [command];
  for (const [flag, value] of usableArgs(args)) {
    parts.push(flag);
    if (value !== null) parts.push(shellArg(shell, value));
  }
  return parts.join(" ");
}

export function RunnerCliCommand(props: Props) {
  const { cloudUrl, workspaceSlug, projectIdentifier, podName, name, workingDir, agent, isUsingBrowserOrigin } = props;
  const { t } = useTranslation();
  const [justCopied, setJustCopied] = useState(false);
  const [shell, setShell] = useState<TShell>("posix");

  const command = useMemo(() => {
    const args: Array<[string, string | null | undefined]> = [
      ["--url", cloudUrl],
      ["--workspace", workspaceSlug],
      ["--project", projectIdentifier],
      ["--pod", podName],
      ["--name", name],
      ["--working-dir", workingDir],
      ["--agent", agent],
    ];
    if (shell === "posix") return posixCommandLines("pidash runner add", args).join("\n");
    return oneLineCommand("pidash runner add", shell, args);
  }, [agent, cloudUrl, name, podName, projectIdentifier, shell, workspaceSlug, workingDir]);

  const shellLabel = (value: TShell): string => {
    if (value === "powershell") return t("runners.add_modal.shell_powershell");
    if (value === "cmd") return t("runners.add_modal.shell_cmd");
    return t("runners.add_modal.shell_posix");
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setJustCopied(true);
      window.setTimeout(() => setJustCopied(false), 2000);
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("runners.toast.error_title"),
        message: t("runners.list.copy_failed"),
      });
    }
  };

  return (
    <div className="border-custom-primary-100/40 bg-custom-primary-100/10 rounded border p-3 text-13 text-primary">
      <p className="text-secondary">{t("runners.add_modal.token_instructions")}</p>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <span className="text-12 text-secondary">{t("runners.add_modal.shell_label")}</span>
        <div className="inline-flex overflow-hidden rounded border border-subtle bg-layer-1">
          {SHELL_OPTIONS.map((option) => (
            <button
              key={option}
              type="button"
              aria-pressed={shell === option}
              onClick={() => setShell(option)}
              className={`border-r border-subtle px-2 py-1 text-12 last:border-r-0 ${
                shell === option ? "bg-custom-primary-100 text-white" : "text-secondary hover:text-primary"
              }`}
            >
              {shellLabel(option)}
            </button>
          ))}
        </div>
      </div>
      <pre className="font-mono mt-2 rounded border border-subtle bg-layer-1 p-2 text-11 whitespace-pre-wrap text-primary select-all">
        {command}
      </pre>
      {isUsingBrowserOrigin && (
        <p className="mt-2 text-12 text-secondary">{t("runners.add_modal.cloud_url_origin_warning")}</p>
      )}
      <div className="mt-2">
        <Button size="sm" onClick={copy}>
          {justCopied ? t("runners.add_modal.copied") : t("runners.add_modal.copy_command")}
        </Button>
      </div>
    </div>
  );
}
