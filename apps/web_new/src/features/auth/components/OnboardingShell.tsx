// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Onboarding shell (AUTH-029, AUTH-043). The stable composition every step
// mounts inside: the progress strip plus fraction, the header row (back
// chevron, brand, account menu with the AUTH-018 switch-account entry),
// and the step container that scrolls to top on every step change. Later
// slices mount their steps here without reworking it. Written from
// parity/specs/auth.md.

import { Avatar, Button, IconButton, Menu } from "@pidash/kit";
import { ChevronLeft } from "lucide-react";
import * as React from "react";

import type { OnboardingStep } from "../onboarding.js";
import { SwitchAccountDialog } from "./SwitchAccountDialog.js";

export interface OnboardingShellProps {
  step: OnboardingStep;
  /** Current step over the visible total. */
  progress: { current: number; total: number };
  canGoBack: boolean;
  onBack: () => void;
  accountName: string;
  accountAvatarUrl: string | null;
  accountEmail: string;
  children: React.ReactNode;
}

export function OnboardingShell({
  step,
  progress,
  canGoBack,
  onBack,
  accountName,
  accountAvatarUrl,
  accountEmail,
  children,
}: OnboardingShellProps): React.ReactElement {
  const [switchOpen, setSwitchOpen] = React.useState(false);
  const scrollRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    const container = scrollRef.current;
    if (container) container.scrollTop = 0;
  }, [step]);

  const percent = progress.total === 0 ? 0 : (progress.current / progress.total) * 100;

  return (
    <div className="flex min-h-screen flex-col bg-(--bg)">
      <header className="flex flex-col">
        <div
          role="progressbar"
          aria-label="Onboarding progress"
          aria-valuenow={progress.current}
          aria-valuemin={1}
          aria-valuemax={progress.total}
          className="h-1.5 w-full bg-(--subtle)"
        >
          <div className="h-full bg-(--accent)" style={{ width: `${percent}%` }} />
        </div>
        <div className="flex w-full items-center justify-between gap-(--space-4) px-(--space-6) py-(--space-3)">
          <div className="flex items-center gap-(--space-2)">
            {canGoBack ? (
              <IconButton label="Back" onClick={onBack}>
                <ChevronLeft size={20} aria-hidden="true" />
              </IconButton>
            ) : null}
            <span className="text-emphasis text-(--text)">Pi Dash</span>
          </div>
          <div className="flex items-center gap-(--space-4)">
            <span className="text-caption text-(--text-muted)" aria-hidden="true">
              {progress.current}/{progress.total}
            </span>
            <Menu
              trigger={
                <Button variant="ghost" aria-label={`Account: ${accountName}`}>
                  <span className="flex min-w-0 items-center gap-(--space-2)">
                    <Avatar name={accountName} {...(accountAvatarUrl ? { src: accountAvatarUrl } : {})} size="small" />
                    <span className="text-emphasis max-w-40 truncate text-(--text)">{accountName}</span>
                  </span>
                </Button>
              }
              items={[{ label: "Use a different email address", onSelect: () => setSwitchOpen(true) }]}
            />
          </div>
        </div>
      </header>
      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-md px-(--space-6) py-(--space-8)">{children}</div>
      </div>
      <SwitchAccountDialog open={switchOpen} onOpenChange={setSwitchOpen} email={accountEmail} />
    </div>
  );
}
