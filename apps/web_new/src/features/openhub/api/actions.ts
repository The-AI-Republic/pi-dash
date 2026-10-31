// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Card flows (written from parity/specs/cloud.md §6,
// CLOUD-024/025/027/028/031): install-then-connect with the catalog
// refreshing before the connect step starts, removal, and direct
// connects. Outcomes report through distinct notices; provider legs
// run in a synchronously reserved tab, key legs in the credential
// dialog the catalog renders from `dialog`.
import type { QueryClient } from "@tanstack/react-query";
import { useQueryClient } from "@tanstack/react-query";
import {
  appOAuthBeginUrl,
  getAppAuthStatus,
  installApp,
  submitAppApiKey,
  uninstallApp,
  type OpenhubApp,
  type OpenhubAuthStatus,
  type OpenhubStatus,
} from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifyInfo, notifySuccess } from "../../../shared/shell/index.js";
import { openhubKeys } from "./keys.js";
import { appMechanism, connectReturnPath, hubAppUrl, reserveConnectTab, type AppMechanism } from "../lib/connect.js";
import { writeFailureMessage } from "../lib/errors.js";

/** Success notice naming the app (CLOUD-024). */
export function installedTitle(appName: string): string {
  return `Installed ${appName}`;
}

/** Install rejected: connecting never starts (CLOUD-024). */
export const INSTALL_FAILED_TITLE = "Install failed";

/** Committed but the connect leg failed; any reserved tab is closed (CLOUD-024). */
export const INSTALLED_UNCONNECTED_TITLE = "Installed, but not connected";

/** A blocked popup explains itself instead of failing silently (CLOUD-025). */
export const POPUP_BLOCKED_DESCRIPTION =
  "The sign-in tab was blocked by a pop-up blocker. Allow pop-ups for this site and try connecting again.";

/** The hub base was refused, so the tab closed unopened (CLOUD-031). */
export const HUB_REFUSED_DESCRIPTION = "The app hub address was refused, so the app stayed unconnected.";

/** Removal commits, then reports (CLOUD-024). */
export function uninstalledTitle(appName: string): string {
  return `Uninstalled ${appName}`;
}

/** Removal rejected (CLOUD-024). */
export const UNINSTALL_FAILED_TITLE = "Uninstall failed";

/** Credential submit accepted (CLOUD-027). */
export function connectedTitle(appName: string): string {
  return `Connected ${appName}`;
}

/** A connect leg that never produced a connection (CLOUD-027). */
export const COULD_NOT_CONNECT_TITLE = "Could not connect";

/** Re-read every catalog window for one workspace. */
export async function refreshMarketplace(queryClient: QueryClient, workspaceSlug: string): Promise<void> {
  await queryClient.invalidateQueries({ queryKey: openhubKeys.marketplace(workspaceSlug) });
}

export interface ConnectDialogState {
  app: OpenhubApp;
  auth: OpenhubAuthStatus;
}

export interface AppCardActions {
  /** Install, refresh, then connect (CLOUD-024/025/027/031). */
  install: (app: OpenhubApp) => Promise<void>;
  /** Remove, refresh, then report (CLOUD-024). */
  remove: (app: OpenhubApp) => Promise<void>;
  /** Connect an installed app without reinstalling. */
  connect: (app: OpenhubApp) => Promise<void>;
  /** Submit dialog credentials; true closes, false stays open (CLOUD-027). */
  submitCredentials: (app: OpenhubApp, fields: Record<string, string>) => Promise<boolean>;
  /** The requested credential dialog, if any. */
  dialog: ConnectDialogState | null;
  /** Close the dialog; the list refreshes either way (CLOUD-027). */
  closeDialog: () => void;
}

export function useAppCardActions(workspaceSlug: string): AppCardActions {
  const queryClient = useQueryClient();
  const [dialog, setDialog] = React.useState<ConnectDialogState | null>(null);

  const statusHubUrl = (): string | undefined =>
    queryClient.getQueryData<OpenhubStatus>(openhubKeys.status(workspaceSlug))?.hub_url;

  const finishProviderConnect = (
    app: OpenhubApp,
    auth: OpenhubAuthStatus,
    tab: Window | null,
    failureTitle: string
  ): void => {
    if (tab === null) {
      notifyError(failureTitle, POPUP_BLOCKED_DESCRIPTION);
      return;
    }
    if (auth.status === "connected") {
      const hub = hubAppUrl(statusHubUrl(), app.slug, app.appId);
      if (hub === null) {
        tab.close();
        notifyError(failureTitle, HUB_REFUSED_DESCRIPTION);
        return;
      }
      tab.location.href = hub;
      return;
    }
    tab.location.href = appOAuthBeginUrl(getAppClient().baseUrl, app.appId, connectReturnPath(window.location));
  };

  const install = async (app: OpenhubApp): Promise<void> => {
    const mechanism: AppMechanism = appMechanism(app.authType);
    // Synchronous with the click: a tab opened after the awaits below
    // would no longer be attributable to it (CLOUD-025).
    const tab = mechanism === "provider" ? reserveConnectTab(window) : null;
    try {
      await installApp(getAppClient(), app.appId);
    } catch (error) {
      tab?.close();
      notifyError(INSTALL_FAILED_TITLE, writeFailureMessage(error));
      return;
    }
    // The commit refreshes before connecting, so a later failure reads
    // as not-connected rather than not-installed (CLOUD-024).
    await refreshMarketplace(queryClient, workspaceSlug);
    // Every install reads auth status, mechanism-less apps included:
    // the oracle pins commit-then-refresh-then-status (CLOUD-024).
    let auth: OpenhubAuthStatus;
    try {
      auth = await getAppAuthStatus(getAppClient(), app.appId);
    } catch (error) {
      tab?.close();
      notifyError(INSTALLED_UNCONNECTED_TITLE, writeFailureMessage(error));
      return;
    }
    if (mechanism === "none") {
      notifySuccess(installedTitle(app.name));
      return;
    }
    if (mechanism === "key") {
      if (auth.status !== "connected") setDialog({ app, auth });
      else notifySuccess(installedTitle(app.name));
      return;
    }
    finishProviderConnect(app, auth, tab, INSTALLED_UNCONNECTED_TITLE);
  };

  const remove = async (app: OpenhubApp): Promise<void> => {
    try {
      await uninstallApp(getAppClient(), app.appId);
    } catch (error) {
      notifyError(UNINSTALL_FAILED_TITLE, writeFailureMessage(error));
      return;
    }
    await refreshMarketplace(queryClient, workspaceSlug);
    notifyInfo(uninstalledTitle(app.name));
  };

  const connect = async (app: OpenhubApp): Promise<void> => {
    const mechanism: AppMechanism = appMechanism(app.authType);
    const tab = mechanism === "provider" ? reserveConnectTab(window) : null;
    let auth: OpenhubAuthStatus;
    try {
      auth = await getAppAuthStatus(getAppClient(), app.appId);
    } catch (error) {
      tab?.close();
      notifyError(COULD_NOT_CONNECT_TITLE, writeFailureMessage(error));
      return;
    }
    if (mechanism === "key" && auth.status !== "connected") {
      setDialog({ app, auth });
      return;
    }
    if (mechanism === "provider") {
      finishProviderConnect(app, auth, tab, COULD_NOT_CONNECT_TITLE);
      return;
    }
    notifySuccess(connectedTitle(app.name));
    await refreshMarketplace(queryClient, workspaceSlug);
  };

  const submitCredentials = async (app: OpenhubApp, fields: Record<string, string>): Promise<boolean> => {
    try {
      await submitAppApiKey(getAppClient(), app.appId, { fields });
    } catch (error) {
      notifyError(COULD_NOT_CONNECT_TITLE, writeFailureMessage(error));
      return false;
    }
    notifySuccess(connectedTitle(app.name));
    return true;
  };

  const closeDialog = (): void => {
    setDialog(null);
    // The install already committed server-side, so every close
    // re-reads: the card must stop offering installation (CLOUD-027).
    void refreshMarketplace(queryClient, workspaceSlug);
  };

  return { install, remove, connect, submitCredentials, dialog, closeDialog };
}
