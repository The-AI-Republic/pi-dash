// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// CLI-install onboarding step (AUTH-030, browser builds only). Per-OS
// install guidance with the current platform preselected, a copy action
// per command block, and two continue actions that advance identically
// and store nothing. Written from parity/specs/auth.md.

import { Button } from "@pidash/kit";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { CLI_INSTALL_TABS, copyInstallCommand, detectCliInstallTab, type CliInstallTab } from "../onboarding.js";

export const CLI_COPY_FAILED_TITLE = "Could not copy the command";

export interface CliInstallStepProps {
  onAdvance: () => void;
}

export function CliInstallStep({ onAdvance }: CliInstallStepProps): React.ReactElement {
  const [tab, setTab] = React.useState<CliInstallTab>(() => detectCliInstallTab(navigator.userAgent));
  const [copiedShell, setCopiedShell] = React.useState<string | null>(null);

  const copy = async (shell: string, command: string): Promise<void> => {
    try {
      await copyInstallCommand(command);
      setCopiedShell(shell);
    } catch {
      notifyError(CLI_COPY_FAILED_TITLE, "Copy it manually from the command block.");
    }
  };

  const active = CLI_INSTALL_TABS[tab];

  return (
    <section aria-labelledby="cli-install-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="cli-install-heading" className="text-h2 text-(--text)">
          Connect your AI agents with Pi Dash
        </h1>
        <p className="text-body text-(--text-muted)">
          The Pi Dash CLI is the link between your AI agents and Pi Dash: it is how you hand tasks to those agents. It
          has to live on whichever machine hosts your coding agent, because that is where the work happens.
        </p>
      </div>

      <div role="note" className="text-body rounded-(--radius-control) bg-(--subtle) p-(--space-4) text-(--text)">
        Pi Dash runs on the CLI plus coding agents from end to end — installing now saves a round trip later.
      </div>

      <div role="group" aria-label="Operating system" className="flex gap-(--space-2)">
        {(Object.keys(CLI_INSTALL_TABS) as CliInstallTab[]).map((key) => (
          <Button
            key={key}
            variant={key === tab ? "secondary" : "ghost"}
            aria-pressed={key === tab}
            onClick={() => setTab(key)}
          >
            {CLI_INSTALL_TABS[key].label}
          </Button>
        ))}
      </div>

      <div className="flex flex-col gap-(--space-4)">
        {active.commands.map((entry) => (
          <div key={entry.shell} className="flex flex-col gap-(--space-2)">
            {entry.hint ? <p className="text-body text-(--text-muted)">{entry.hint}</p> : null}
            <div className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-4)">
              <div className="flex items-center justify-between gap-(--space-2)">
                <span className="text-emphasis text-(--text)">{entry.shell}</span>
                <Button
                  variant="ghost"
                  size="small"
                  onClick={() => void copy(entry.shell, entry.command)}
                  aria-label={
                    copiedShell === entry.shell ? `Copied ${entry.shell} command` : `Copy ${entry.shell} command`
                  }
                >
                  {copiedShell === entry.shell ? "Copied" : "Copy"}
                </Button>
              </div>
              <pre className="text-body overflow-x-auto text-(--text)">
                <code>{entry.command}</code>
              </pre>
            </div>
          </div>
        ))}
      </div>

      <div className="flex flex-col gap-(--space-2)">
        <Button variant="primary" size="large" onClick={onAdvance}>
          Continue
        </Button>
        <Button variant="ghost" onClick={onAdvance}>
          Skip for now
        </Button>
      </div>
    </section>
  );
}
