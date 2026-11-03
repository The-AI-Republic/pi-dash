// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Integrations tab (parity/specs/profile.md §11, PROF-025). A member with
// no admin workspace gets the explanatory view; an admin picks one of
// their workspaces and sees that workspace's GitHub App connection. The
// install handoff leaves the app for the host, and the return marker is
// turned into a notice, a workspace selection and an address with the
// marker keys removed. Nothing here polls, binds keys or accepts a drop
// (PROF-027); the panel reads the signed-in member's admin scope only
// (PROF-028).

import { Skeleton } from "@pidash/kit";
import { useRouterState } from "@tanstack/react-router";
import * as React from "react";

import {
  CALLBACK_FAILED_TITLE,
  CALLBACK_OK_MESSAGE,
  CALLBACK_OK_TITLE,
  INTEGRATIONS_TAB_HEADING,
  INTEGRATIONS_TAB_INTRO,
  NO_ADMIN_COPY,
  NO_ADMIN_HEADING,
  WORKSPACE_LABEL,
} from "../lib/copy.js";
import { readInstallCallback, scrubbedAddress } from "../lib/callback.js";
import { selectedWorkspace } from "../lib/panel.js";
import { useGithubAppStatus, useRefreshGithubInstall, useStartGithubInstall } from "../api.js";
import { GithubAppPanel } from "./GithubAppPanel.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";

const WORKSPACE_SELECT_CLASSES =
  "h-(--control-height) w-full max-w-sm rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-body text-(--text) focus:border-(--accent) focus:outline-none";

export function IntegrationsSettingsPage(): React.ReactElement {
  const status = useGithubAppStatus();
  const startInstall = useStartGithubInstall();
  const refresh = useRefreshGithubInstall();
  const search = useRouterState({ select: (state) => state.location.searchStr });
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [pickedSlug, setPickedSlug] = React.useState<string | null>(null);
  const handledCallback = React.useRef<string | null>(null);

  // One notice and one scrub per distinct marker; a re-render never
  // repeats either. The address rewrite happens in place, so the shell's
  // own navigation state is untouched.
  React.useEffect(() => {
    const callback = readInstallCallback(search);
    if (callback === null) return;
    if (handledCallback.current === search) return;
    handledCallback.current = search;
    if (callback.ok) {
      notifySuccess(CALLBACK_OK_TITLE, CALLBACK_OK_MESSAGE);
      if (callback.workspaceSlug !== null) {
        setPickedSlug(callback.workspaceSlug);
        void status.refetch();
      }
    } else {
      notifyError(CALLBACK_FAILED_TITLE, callback.message);
    }
    window.history.replaceState(null, "", scrubbedAddress(pathname, search, window.location.hash));
  }, [search, pathname, status]);

  const workspace = selectedWorkspace(status.data, pickedSlug);

  return (
    <div data-testid="integrations-settings-page" className="flex max-w-3xl flex-col gap-(--space-6)">
      <header className="flex flex-col gap-(--space-1)">
        <h2 className="text-h3 text-(--text)">{INTEGRATIONS_TAB_HEADING}</h2>
        <p className="text-body text-(--text-muted)">{INTEGRATIONS_TAB_INTRO}</p>
      </header>

      {status.data === undefined ? (
        <div data-testid="integrations-settings-loading" aria-busy="true" className="flex flex-col gap-(--space-3)">
          <Skeleton className="h-20 w-full" />
          <Skeleton className="h-48 w-full" />
        </div>
      ) : workspace === undefined ? (
        <div
          data-testid="integrations-no-admin"
          className="flex flex-col gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-4) py-(--space-4)"
        >
          <h3 className="text-body font-medium text-(--text)">{NO_ADMIN_HEADING}</h3>
          <p className="text-meta text-(--text-muted)">{NO_ADMIN_COPY}</p>
        </div>
      ) : (
        <>
          <div className="flex flex-col gap-(--space-2)">
            <label htmlFor="integrations-workspace" className="text-body font-medium text-(--text)">
              {WORKSPACE_LABEL}
            </label>
            <select
              id="integrations-workspace"
              className={WORKSPACE_SELECT_CLASSES}
              value={workspace.slug}
              onChange={(event) => setPickedSlug(event.target.value)}
            >
              {status.data.workspaces.map((row) => (
                <option key={row.id} value={row.slug}>
                  {row.name}
                </option>
              ))}
            </select>
          </div>

          <GithubAppPanel
            status={status.data}
            workspace={workspace}
            installing={startInstall.isPending}
            refreshing={refresh.isPending}
            onInstall={() => {
              startInstall.mutate(workspace.slug, {
                onSuccess: (url) => {
                  window.location.assign(url);
                },
              });
            }}
            onRefresh={() => refresh.mutate(workspace.slug)}
          />
        </>
      )}
    </div>
  );
}
