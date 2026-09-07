/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { AlertTriangle, Check, Copy, Terminal } from "lucide-react";
// pi dash imports
import { Button } from "@pi-dash/propel/button";
import { Tabs } from "@pi-dash/propel/tabs";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { EOnboardingSteps } from "@pi-dash/types";
// hooks
import { usePlatformOS } from "@/hooks/use-platform-os";
// local components
import { CommonOnboardingHeader } from "../common";

type Props = {
  handleStepChange: (step: EOnboardingSteps, skipInvites?: boolean) => void;
};

type TPlatformKey = "unix" | "windows";

type TInstallCommand = {
  id: string;
  /** Shell the command is meant to be pasted into. */
  shell: string;
  command: string;
  /** Rendered above the block when a platform ships more than one shell. */
  hint?: string;
};

const RELEASE_BASE = "https://github.com/The-AI-Republic/pi-dash/releases/latest/download";

/**
 * These point at the `install.sh` / `install.ps1` *wrappers*, not the
 * `pidash-installer.*` scripts underneath them. The wrappers install the
 * binary and then run `pidash auth login`, so a user who pastes one of
 * these lands connected rather than holding an unauthenticated binary.
 */
const PLATFORMS: Record<TPlatformKey, { label: string; commands: TInstallCommand[] }> = {
  unix: {
    label: "macOS / Linux",
    commands: [
      {
        id: "unix-sh",
        shell: "Terminal",
        command: `curl --proto '=https' --tlsv1.2 -LsSf ${RELEASE_BASE}/install.sh | sh`,
      },
    ],
  },
  windows: {
    label: "Windows",
    commands: [
      {
        id: "windows-powershell",
        shell: "PowerShell",
        command: `irm ${RELEASE_BASE}/install.ps1 | iex`,
      },
      {
        id: "windows-cmd",
        shell: "Command Prompt",
        hint: "Using Command Prompt instead?",
        command: `powershell -c "irm ${RELEASE_BASE}/install.ps1 | iex"`,
      },
    ],
  },
};

/**
 * `navigator.clipboard` is only exposed on secure origins, so a self-hosted
 * instance served over plain HTTP would otherwise fail silently here. Fall
 * back to a detached textarea + `execCommand` before giving up.
 */
async function copyToClipboard(value: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();

  try {
    if (!document.execCommand("copy")) throw new Error("copy command was rejected");
  } finally {
    document.body.removeChild(textarea);
  }
}

function CommandBlock({ command }: { command: TInstallCommand }) {
  const [hasCopied, setHasCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await copyToClipboard(command.command);
      setHasCopied(true);
      window.setTimeout(() => setHasCopied(false), 2000);
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Couldn't copy",
        message: "Copying isn't available here. Select the command and copy it manually.",
      });
    }
  };

  return (
    <div className="flex flex-col gap-3">
      {command.hint && <p className="text-body-sm-regular text-tertiary">{command.hint}</p>}
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-body-sm-semibold text-placeholder">
          <Terminal className="size-4" />
          <span>{command.shell}</span>
        </div>
        <Button
          variant="secondary"
          size="lg"
          className="shrink-0"
          onClick={handleCopy}
          prependIcon={hasCopied ? <Check /> : <Copy />}
        >
          {hasCopied ? "Copied" : "Copy command"}
        </Button>
      </div>
      <pre className="max-w-full overflow-x-auto rounded-lg border border-subtle bg-surface-2 p-4 text-left">
        <code className="font-mono text-13 leading-5 break-words whitespace-pre-wrap text-secondary">
          {command.command}
        </code>
      </pre>
    </div>
  );
}

export function CliInstallStep({ handleStepChange }: Props) {
  const { platform } = usePlatformOS();
  // Preselect the tab matching the browser's OS, but leave it switchable —
  // people routinely set up a runner on a machine other than the one they
  // are reading this on.
  const [selectedPlatform, setSelectedPlatform] = useState<TPlatformKey>("unix");

  useEffect(() => {
    if (platform === "Windows") setSelectedPlatform("windows");
  }, [platform]);

  const continueToProfile = () => handleStepChange(EOnboardingSteps.CLI_INSTALL);

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

      <Tabs
        value={selectedPlatform}
        onValueChange={(value) => setSelectedPlatform(value as TPlatformKey)}
        className="h-auto gap-6"
      >
        <Tabs.List>
          {Object.entries(PLATFORMS).map(([key, { label }]) => (
            <Tabs.Trigger key={key} value={key}>
              {label}
            </Tabs.Trigger>
          ))}
          <Tabs.Indicator />
        </Tabs.List>
        {Object.entries(PLATFORMS).map(([key, { commands }]) => (
          <Tabs.Content key={key} value={key} className="flex flex-col gap-6">
            {commands.map((command) => (
              <CommandBlock key={command.id} command={command} />
            ))}
          </Tabs.Content>
        ))}
      </Tabs>

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
