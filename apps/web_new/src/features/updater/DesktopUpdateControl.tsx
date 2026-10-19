// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar update detail (DESK-018). The shell owns the mount point beside
// the account area (SHELL-102); this control owns everything inside it:
// asking what waits, naming both versions, blocking a second install while
// one downloads, and reporting a failure in place with a retry. It renders
// nothing when no update waits, which is also the web bundle's whole story.

import { ArrowUpCircle } from "lucide-react";
import { Button, Spinner, Tooltip } from "@pidash/kit";
import * as React from "react";

import { platform } from "../../core/platform/index.js";
import { useUpdaterState, type UpdaterState } from "./state.js";

/** Accessible name of the install control; the release follows. */
export const UPDATE_CONTROL_LABEL = "Update to Pi Dash";

/** Retry action shown after a failed install. */
export const UPDATE_RETRY_LABEL = "Try again";

export function updateControlName(version: string): string {
  return `${UPDATE_CONTROL_LABEL} ${version}`;
}

export function updateReadyHint(version: string, currentVersion: string): string {
  return `Update to ${version} — you're on ${currentVersion}. The app restarts when it's done.`;
}

export function updateInstallingHint(version: string): string {
  return `Installing ${version}…`;
}

export interface DesktopUpdateControlViewProps {
  state: UpdaterState;
}

/**
 * Presentational control: all shell traffic arrives through `state`, so
 * tests drive every branch without a native bridge.
 */
export function DesktopUpdateControlView({ state }: DesktopUpdateControlViewProps): React.ReactElement | null {
  const { update, requesting, failure, install } = state;
  if (update === null) return null;
  const busy = requesting || update.installing;
  const name = updateControlName(update.version);
  const currentVersionId = React.useId();

  return (
    <div data-testid="desktop-update-control" className="flex shrink-0 items-center">
      <Tooltip
        content={busy ? updateInstallingHint(update.version) : updateReadyHint(update.version, update.currentVersion)}
      >
        <Button
          variant="secondary"
          size="small"
          aria-label={name}
          aria-describedby={currentVersionId}
          disabled={busy}
          aria-busy={busy || undefined}
          onClick={install}
          className="max-w-40"
        >
          {busy ? (
            <Spinner size="small" label={updateInstallingHint(update.version)} />
          ) : (
            <ArrowUpCircle size={14} strokeWidth={1.75} aria-hidden="true" />
          )}
          <span className="truncate">{name}</span>
        </Button>
      </Tooltip>
      {/* Both versions stay on the accessible surface even where the
          sidebar is too narrow to print them. */}
      <span id={currentVersionId} className="sr-only">
        {`You are on ${update.currentVersion}`}
      </span>
      {failure === null ? null : (
        <div role="alert" data-testid="desktop-update-failure" className="flex items-center gap-1 pl-(--space-1)">
          <span className="text-meta text-(--danger)">{failure}</span>
          <Button variant="ghost" size="small" onClick={install}>
            {UPDATE_RETRY_LABEL}
          </Button>
        </div>
      )}
    </div>
  );
}

/** The control mounted by the workspace shell; wired to the desktop platform. */
export function DesktopUpdateControl(): React.ReactElement | null {
  const state = useUpdaterState(platform.updates);
  return <DesktopUpdateControlView state={state} />;
}
