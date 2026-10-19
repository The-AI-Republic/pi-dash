// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Manual enrolment panel (RUN-008). The generated command is shown in a
// selectable block with a shell selector and a copy control that confirms
// briefly. A clipboard failure toasts instead of failing silently. The
// origin note appears only when the build carries no API base URL.

import { Button } from "@pidash/kit";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { cn } from "@pidash/kit";
import { buildRunnerCommand, SHELL_CHOICES, type CommandFields, type ShellKind } from "./command.js";

export const COPY_FAILED_TITLE = "Error!";
export const COPY_FAILED_MESSAGE = "Could not copy to clipboard";
export const COPY_CONFIRM_MS = 2_000;
export const ORIGIN_NOTE =
  "Using the current browser origin as the cloud URL because VITE_API_BASE_URL is not configured.";

export interface CommandPanelProps {
  fields: CommandFields;
  /** True when the origin was not configured and the browser origin stands in. */
  originFallback: boolean;
}

export function AddRunnerCommandPanel({ fields, originFallback }: CommandPanelProps): React.ReactElement {
  const [shell, setShell] = React.useState<ShellKind>("posix");
  const [copied, setCopied] = React.useState(false);
  const confirmTimer = React.useRef<number | null>(null);
  const mounted = React.useRef(true);

  React.useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (confirmTimer.current !== null) window.clearTimeout(confirmTimer.current);
    };
  }, []);

  const command = React.useMemo(() => buildRunnerCommand(shell, fields), [fields, shell]);

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      if (confirmTimer.current !== null) window.clearTimeout(confirmTimer.current);
      confirmTimer.current = window.setTimeout(() => {
        if (mounted.current) setCopied(false);
      }, COPY_CONFIRM_MS);
    } catch {
      notifyError(COPY_FAILED_TITLE, COPY_FAILED_MESSAGE);
    }
  }

  return (
    <div
      data-testid="add-runner-command-panel"
      className="flex flex-col gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-4)"
    >
      <p className="text-body text-(--text-muted)">Run this on the machine that will host the runner:</p>
      <div className="flex flex-wrap items-center gap-(--space-2)">
        <span className="text-meta text-(--text-muted)">Shell</span>
        <div
          role="group"
          aria-label="Shell"
          className="inline-flex overflow-hidden rounded-(--radius-control) border border-(--border) bg-(--bg)"
        >
          {SHELL_CHOICES.map((choice) => (
            <button
              key={choice.kind}
              type="button"
              aria-pressed={shell === choice.kind}
              onClick={() => setShell(choice.kind)}
              className={cn(
                "text-meta cursor-pointer border-r border-(--border) px-(--space-3) py-(--space-1) last:border-r-0",
                shell === choice.kind ? "bg-(--accent) text-(--on-accent)" : "text-(--text-muted) hover:text-(--text)"
              )}
            >
              {choice.label}
            </button>
          ))}
        </div>
      </div>
      <pre
        data-testid="add-runner-command-text"
        className="text-meta font-mono overflow-x-auto rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-3) whitespace-pre-wrap text-(--text) select-all"
      >
        {command}
      </pre>
      {originFallback ? <p className="text-meta text-(--text-muted)">{ORIGIN_NOTE}</p> : null}
      <div>
        <Button size="small" onClick={() => void copy()}>
          {copied ? "Copied!" : "Copy command"}
        </Button>
      </div>
    </div>
  );
}
