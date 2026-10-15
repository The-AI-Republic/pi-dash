// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin data hooks (Data layer page). Components read the instance
// bootstrap and the native-POST forgery token through these, never
// through raw client calls.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { cloudSignInUrl, updateInstance, type InstanceBootstrap, type InstancePatch } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../core/api/client.js";
import { notifySuccess } from "../../shared/shell/index.js";
import type { CloudEntrySessionState } from "./cloudEntry.js";
import {
  GENERAL_SAVE_FAILURE_LOG,
  GENERAL_SAVE_SUCCESS_MESSAGE,
  GENERAL_SAVE_SUCCESS_TITLE,
  mergeGeneralSettingsIntoBootstrap,
} from "./generalSettings.js";
import {
  adminCsrfQueryOptions,
  adminKeys,
  adminSessionQueryOptions,
  cloudAuthProbeQueryOptions,
  instanceAdminsQueryOptions,
  instanceBootstrapQueryOptions,
} from "./queries.js";

/**
 * The single instance-info read the entry gate branches on. Pending
 * while unresolved, errored when the fetch fails, settled after —
 * never refetched behind the screens' backs.
 */
export function useInstanceBootstrap() {
  return useQuery(instanceBootstrapQueryOptions(getAppClient()));
}

/** The forgery token the native setup/sign-in POSTs carry. */
export function useAdminCsrfToken() {
  return useQuery(adminCsrfQueryOptions(getAppClient()));
}

/** The admins list the general page reads its first email from. */
export function useInstanceAdmins() {
  return useQuery(instanceAdminsQueryOptions(getAppClient()));
}

/**
 * The general-settings shared save (ADM-017): one PATCH carrying the
 * name plus the telemetry flag. Success folds the answer into the
 * cached bootstrap and raises the success notice; failure only logs —
 * the accepted log-only quirk, so no notice and the server untouched.
 */
export function useUpdateGeneralSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (patch: InstancePatch) => updateInstance(getAppClient(), patch),
    onSuccess: (updated) => {
      queryClient.setQueryData<InstanceBootstrap>(adminKeys.bootstrap, (old) =>
        old === undefined ? old : mergeGeneralSettingsIntoBootstrap(old, updated)
      );
      notifySuccess(GENERAL_SAVE_SUCCESS_TITLE, GENERAL_SAVE_SUCCESS_MESSAGE);
    },
    onError: (error) => {
      console.error(GENERAL_SAVE_FAILURE_LOG, error);
    },
  });
}

/**
 * The admin login tri-state the guards branch on: pending while the
 * check is unresolved, then admin, anonymous, or forbidden — or errored
 * when the check itself fails, which guards read as anonymous.
 */
export function useAdminSession() {
  return useQuery(adminSessionQueryOptions(getAppClient()));
}

/**
 * The cloud entry's one-shot probe (ADM-079, cloud only). Enabled
 * exactly on a genuine admin-check 403: the anonymous read already
 * proves no session, successes need no classification, and transient
 * check failures retry without probing.
 */
export function useCloudAuthProbe(session: CloudEntrySessionState) {
  return useQuery({
    ...cloudAuthProbeQueryOptions(getAppClient()),
    enabled: session === "forbidden",
  });
}

/**
 * The cloud OIDC entry for a deep-link return (ADM-076, cloud only):
 * the sign-in URL against the app API base carrying the relative
 * return_to the bounce round-trips.
 */
export function useCloudSignInUrl(returnTo: string): string {
  return cloudSignInUrl(getAppClient().baseUrl, returnTo);
}

/**
 * Re-run the cloud entry's admin check (ADM-078, cloud only). Reset
 * clears both verdicts to pending so the loader shows while the check
 * re-issues; a fresh 403 re-probes because the probe resets too.
 */
export function useCloudEntryRetry(): () => void {
  const queryClient = useQueryClient();
  return React.useCallback(() => {
    void queryClient.resetQueries({ queryKey: adminKeys.cloudProbe });
    void queryClient.resetQueries({ queryKey: adminKeys.session });
  }, [queryClient]);
}
