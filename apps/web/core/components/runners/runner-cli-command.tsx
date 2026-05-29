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
};

function shellArg(value: string): string {
  const trimmed = value.trim();
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(trimmed)) return trimmed;
  return `'${trimmed.replaceAll("'", "'\\''")}'`;
}

function commandLines(command: string, args: Array<[string, string | null | undefined]>): string[] {
  const usableArgs = args
    .map(([flag, value]) => [flag, value === null ? null : value?.trim()] as const)
    .filter((arg): arg is readonly [string, string | null] => arg[1] === null || Boolean(arg[1]));

  if (usableArgs.length === 0) return [command];

  return [
    `${command} \\`,
    ...usableArgs.map(([flag, value], index) => {
      const suffix = index === usableArgs.length - 1 ? "" : " \\";
      if (value === null) return `  ${flag}${suffix}`;
      return `  ${flag} ${shellArg(value)}${suffix}`;
    }),
  ];
}

export function RunnerCliCommand(props: Props) {
  const { cloudUrl, workspaceSlug, projectIdentifier, podName, name, workingDir, agent } = props;
  const { t } = useTranslation();
  const [justCopied, setJustCopied] = useState(false);

  const command = useMemo(() => {
    return commandLines("pidash runner add", [
      ["--url", cloudUrl],
      ["--workspace", workspaceSlug],
      ["--project", projectIdentifier],
      ["--pod", podName],
      ["--name", name],
      ["--working-dir", workingDir],
      ["--agent", agent],
    ]).join("\n");
  }, [agent, cloudUrl, name, podName, projectIdentifier, workspaceSlug, workingDir]);

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
      <pre className="font-mono mt-2 rounded border border-subtle bg-layer-1 p-2 text-11 whitespace-pre-wrap text-primary select-all">
        {command}
      </pre>
      <div className="mt-2">
        <Button size="sm" onClick={copy}>
          {justCopied ? t("runners.add_modal.copied") : t("runners.add_modal.copy_command")}
        </Button>
      </div>
    </div>
  );
}
