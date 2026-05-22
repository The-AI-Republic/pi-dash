/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { AlertTriangle, Check, Copy, Terminal } from "lucide-react";
// pi dash imports
import { Button } from "@pi-dash/propel/button";
import { EOnboardingSteps } from "@pi-dash/types";
// local components
import { CommonOnboardingHeader } from "../common";

type Props = {
  handleStepChange: (step: EOnboardingSteps, skipInvites?: boolean) => void;
};

const INSTALL_SCRIPT = `curl -fsSL https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh | sh`;

export function CliInstallStep({ handleStepChange }: Props) {
  const [hasCopied, setHasCopied] = useState(false);

  const continueToProfile = () => handleStepChange(EOnboardingSteps.CLI_INSTALL);
  const copyCommand = async () => {
    await navigator.clipboard.writeText(INSTALL_SCRIPT);
    setHasCopied(true);
    window.setTimeout(() => setHasCopied(false), 2000);
  };

  return (
    <div className="flex flex-col gap-10">
      <CommonOnboardingHeader
        title="Install the Pi Dash CLI."
        description="Run this on the machine where your coding agent will execute work."
      />

      <div className="flex gap-2 text-warning-primary">
        <AlertTriangle className="mt-0.5 size-4 shrink-0" />
        <p className="text-body-sm-medium">
          Important: Pi Dash CLI has to be installed on the dev machine together with AI agents like Claude and Codex to
          make Pi Dash work end to end.
        </p>
      </div>

      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-body-sm-semibold text-placeholder">
            <Terminal className="size-4" />
            <span>Terminal</span>
          </div>
          <Button
            variant="secondary"
            size="lg"
            className="shrink-0"
            onClick={copyCommand}
            prependIcon={hasCopied ? <Check /> : <Copy />}
          >
            {hasCopied ? "Copied" : "Copy command"}
          </Button>
        </div>
        <pre className="max-w-full overflow-x-auto rounded-lg border border-subtle bg-surface-2 p-4 text-left">
          <code className="font-mono text-13 leading-5 break-words whitespace-pre-wrap text-secondary">
            {INSTALL_SCRIPT}
          </code>
        </pre>
      </div>

      <div className="space-y-3">
        <Button variant="ghost" size="xl" className="w-full text-tertiary" onClick={continueToProfile}>
          Skip for now
        </Button>
        <Button variant="primary" size="xl" className="w-full" onClick={continueToProfile}>
          Done, Continue
        </Button>
      </div>
    </div>
  );
}
