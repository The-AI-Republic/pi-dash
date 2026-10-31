// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One workspace's code-hosting connection (parity/specs/profile.md §11,
// PROF-025). The panel names who it is installed for, warns while the
// instance lacks the app configuration, and shows the identifier grid,
// the suspended and last-check banners of a connected installation. The
// install control hands off to the host (reconnect once connected) and
// the refresh control re-checks a live installation without leaving.

import { Badge, Button } from "@pidash/kit";
import { ExternalLink, RefreshCw } from "lucide-react";
import * as React from "react";

import type { GithubAppStatus, GithubAppWorkspace } from "@pidash/api-client";

import {
  CONFIG_MISSING_WARNING,
  GITHUB_PANEL_HEADING,
  LAST_CHECK_FAILED_PREFIX,
  REFRESH_LABEL,
  SUSPENDED_BANNER,
  UNCONNECTED_COPY,
  installLead,
  installedLead,
} from "../lib/copy.js";
import {
  installDisabled,
  installLabel,
  isConnected,
  isSuspended,
  lastCheckError,
  refreshVisible,
  statusFields,
} from "../lib/panel.js";

export interface GithubAppPanelProps {
  status: GithubAppStatus;
  workspace: GithubAppWorkspace;
  onInstall: () => void;
  onRefresh: () => void;
  installing: boolean;
  refreshing: boolean;
}

export function GithubAppPanel({
  status,
  workspace,
  onInstall,
  onRefresh,
  installing,
  refreshing,
}: GithubAppPanelProps): React.ReactElement {
  const installation = workspace.github_app;
  const connected = isConnected(installation);
  const suspended = isSuspended(installation);
  const fields = statusFields(installation);
  const checkError = lastCheckError(installation);
  const disabled = installDisabled(status.configured) || installing;

  return (
    <div
      data-testid="github-app-panel"
      className="flex flex-col gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-4) py-(--space-4)"
    >
      <div className="flex flex-col gap-(--space-3) sm:flex-row sm:items-start sm:justify-between">
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <div className="flex items-center gap-(--space-2)">
            <h3 className="text-body truncate font-medium text-(--text)">{GITHUB_PANEL_HEADING}</h3>
            {connected && !suspended ? (
              <Badge tone="success" dot>
                Live
              </Badge>
            ) : null}
          </div>
          <p className="text-meta text-(--text-muted)">
            {connected
              ? installedLead(installation.account_login || GITHUB_PANEL_HEADING, workspace.name)
              : installLead(workspace.name)}
          </p>
        </div>
        <div className="flex shrink-0 flex-wrap items-center gap-(--space-2)">
          {refreshVisible(installation) ? (
            <Button size="small" loading={refreshing} disabled={refreshing} onClick={onRefresh}>
              <RefreshCw aria-hidden="true" size={14} />
              {REFRESH_LABEL}
            </Button>
          ) : null}
          <Button variant="primary" size="small" loading={installing} disabled={disabled} onClick={onInstall}>
            {installLabel(connected)}
            <ExternalLink aria-hidden="true" size={14} />
          </Button>
        </div>
      </div>

      {status.configured ? null : (
        <p
          role="status"
          className="text-meta rounded-(--radius-control) border border-(--warning) px-(--space-3) py-(--space-2) text-(--text)"
        >
          {CONFIG_MISSING_WARNING}
        </p>
      )}

      {connected ? (
        <div data-testid="github-app-status" className="grid gap-(--space-3) sm:grid-cols-2">
          {fields.map((field) => (
            <div
              key={field.label}
              className="min-w-0 rounded-(--radius-control) bg-(--subtle) px-(--space-3) py-(--space-2)"
            >
              <div className="text-caption text-(--text-muted)">{field.label}</div>
              <div className="text-meta truncate font-medium text-(--text)">{field.value}</div>
            </div>
          ))}
        </div>
      ) : (
        <p className="text-meta text-(--text-muted)">{UNCONNECTED_COPY}</p>
      )}

      {suspended ? (
        <p
          role="alert"
          className="text-meta rounded-(--radius-control) border border-(--danger) px-(--space-3) py-(--space-2) text-(--danger)"
        >
          {SUSPENDED_BANNER}
        </p>
      ) : null}

      {checkError === null ? null : (
        <p
          role="alert"
          className="text-meta rounded-(--radius-control) border border-(--danger) px-(--space-3) py-(--space-2) text-(--danger)"
        >
          {`${LAST_CHECK_FAILED_PREFIX} ${checkError}`}
        </p>
      )}
    </div>
  );
}
