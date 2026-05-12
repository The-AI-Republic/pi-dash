/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import { API_BASE_URL } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IRunnerInvite } from "@pi-dash/types";

type Props = {
  invite: IRunnerInvite;
  /** Optional CLI flags surfaced for parity with the create-runner modal.
   * The host_label / working_dir / agent flags don't change anything
   * server-side — they're CLI-only, so we keep the input simple here. */
  hostLabel?: string;
  workingDir?: string;
  /** Agent kind in the runner CLI's kebab-case value-enum spelling
   * (``codex`` or ``claude-code``). Omitted from the rendered command
   * when unset. */
  agent?: string;
};

/**
 * Displays the ``pidash connect --url ... --token ...`` command for a
 * freshly minted enrollment token. Shared between the add-runner flow
 * and the revive flow so both surface the same install instructions.
 */
export function RunnerEnrollmentCommand(props: Props) {
  const { invite, hostLabel, workingDir, agent } = props;
  const { t } = useTranslation();
  const [origin, setOrigin] = useState("");
  const [justCopied, setJustCopied] = useState(false);

  useEffect(() => {
    if (typeof window !== "undefined") setOrigin(window.location.origin);
  }, []);

  const apiOrigin = API_BASE_URL || origin;

  const enrollmentCommand = useMemo(() => {
    const lines = [`pidash connect \\`, `  --url ${apiOrigin} \\`, `  --token ${invite.enrollment_token}`];
    const optionalArgs: string[] = [];
    if (hostLabel?.trim()) optionalArgs.push(`  --host-label ${hostLabel.trim()}`);
    if (workingDir?.trim()) optionalArgs.push(`  --working-dir ${workingDir.trim()}`);
    if (agent?.trim()) optionalArgs.push(`  --agent ${agent.trim()}`);
    if (optionalArgs.length > 0) {
      lines[lines.length - 1] += " \\";
      for (let i = 0; i < optionalArgs.length; i += 1) {
        const isLast = i === optionalArgs.length - 1;
        lines.push(isLast ? optionalArgs[i] : `${optionalArgs[i]} \\`);
      }
    }
    return lines.join("\n");
  }, [invite.enrollment_token, apiOrigin, hostLabel, workingDir, agent]);

  const copy = async () => {
    if (!enrollmentCommand) return;
    try {
      await navigator.clipboard.writeText(enrollmentCommand);
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
    <div className="border-amber-500/40 bg-amber-500/10 rounded border p-3 text-13 text-primary">
      <div className="font-medium">{t("runners.add_modal.token_warning")}</div>
      <p className="mt-2 text-secondary">{t("runners.add_modal.token_instructions")}</p>
      <pre className="font-mono mt-1 rounded border border-subtle bg-layer-1 p-2 text-11 whitespace-pre-wrap text-primary select-all">
        {enrollmentCommand}
      </pre>
      <div className="mt-2">
        <Button size="sm" onClick={copy}>
          {justCopied ? t("runners.add_modal.copied") : t("runners.add_modal.copy_command")}
        </Button>
      </div>
    </div>
  );
}
