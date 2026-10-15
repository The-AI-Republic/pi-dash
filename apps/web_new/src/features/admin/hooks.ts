// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin data hooks (Data layer page). Components read server data and
// flip the loop toggles through these, never through raw client calls.

import { mutationOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import {
  checkEmailCredentials,
  cloudSignInUrl,
  disableEmailFeature,
  updateInstance,
  updateInstanceConfigurations,
  updateLoopJob,
  type ApiClient,
  type InstanceBootstrap,
  type InstanceConfigurationList,
  type InstancePatch,
  type LoopJobList,
} from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import type { CloudEntrySessionState } from "./cloudEntry.js";
import { BLANKED_MAIL_KEYS, SMTP_ENABLED_KEY, type ConfigValueMap } from "./emailConfig.js";
import { LOOP_TOGGLE_FAILURE_MESSAGE, LOOP_TOGGLE_FAILURE_TITLE } from "./loopJobs.js";
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
  instanceConfigurationsQueryOptions,
  loopJobsQueryOptions,
  mergeConfigurationRows,
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
 * The shared configurations key map every settings page reads. One
 * fetch for the console lifetime; saves merge the answered rows back
 * into it, so pages always render stored state.
 */
export function useInstanceConfigurations() {
  return useQuery(instanceConfigurationsQueryOptions(getAppClient()));
}

/** Console line for a failed configurations write, pinned by the parity scenarios. */
export const CONFIG_SAVE_FAILED_LOG = "Error updating the instance configurations";

/**
 * The shared configurations PATCH (ADM-023, ADM-025): writes a partial
 * string map, folds the answered rows into the cached key map so saved
 * pages turn clean, and logs failures to the console. Pages attach
 * their own notices per mutate call; the hook owns only the shared
 * cache merge and the failure log.
 */
export function useUpdateInstanceConfigurations() {
  const queryClient = useQueryClient();
  return useMutation<InstanceConfigurationList, Error, Record<string, string>>({
    mutationFn: (patch) => updateInstanceConfigurations(getAppClient(), patch),
    onSuccess: (updated) => {
      queryClient.setQueryData<InstanceConfigurationList | undefined>(adminKeys.configurations, (current) =>
        mergeConfigurationRows(current, updated)
      );
    },
    onError: (error) => {
      console.error(CONFIG_SAVE_FAILED_LOG, error);
    },
  });
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

/** The loop jobs list the loop pages render (ADM-051). */
export function useLoopJobs() {
  return useQuery(loopJobsQueryOptions(getAppClient()));
}

export interface LoopJobToggle {
  id: string;
  enabled: boolean;
}

interface ToggleMutationContext {
  rollback: Rollback;
}

export function toggleLoopJobMutationOptions(client: ApiClient, queryClient: QueryClient) {
  return mutationOptions({
    mutationFn: (toggle: LoopJobToggle) => updateLoopJob(client, toggle.id, { enabled: toggle.enabled }),
    onMutate: (toggle: LoopJobToggle): ToggleMutationContext => ({
      rollback: applyOptimisticUpdate<LoopJobList>(queryClient, adminKeys.loopJobs, (previous) =>
        previous?.map((job) => (job.id === toggle.id ? { ...job, enabled: toggle.enabled } : job))
      ),
    }),
    onError: (_error: unknown, _toggle: LoopJobToggle, context: ToggleMutationContext | undefined) => {
      context?.rollback();
      notifyError(LOOP_TOGGLE_FAILURE_TITLE, LOOP_TOGGLE_FAILURE_MESSAGE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: adminKeys.loopJobs });
    },
  });
}

/**
 * Flips one job's enabled flag with an optimistic paint (ADM-052).
 * Silent on success; a failed write rolls back with an error notice
 * and the list revalidates once settled either way.
 */
export function useToggleLoopJob() {
  return useMutation(toggleLoopJobMutationOptions(getAppClient(), useQueryClient()));
}

/** Configuration rows as a value map; the form reads stored values from it. */
export function configValueMap(rows: InstanceConfigurationList | undefined): ConfigValueMap {
  const map: ConfigValueMap = {};
  for (const row of rows ?? []) {
    map[row.key] = row.value;
  }
  return map;
}

/** Save-success notice (ADM-019; wording pinned by the parity scenario). */
export const SMTP_SAVE_TOAST_TITLE = "Success";
export const SMTP_SAVE_TOAST_MESSAGE = "Email Settings updated successfully";
/** Save-failure console line (ADM-019 log-only quirk; pinned by the scenario). */
export const SMTP_SAVE_ERROR_LOG = "Error updating the instance configurations";

/**
 * Persist the SMTP configuration (ADM-019): PATCH the mail keys, fold the
 * answered rows back into the shared cache, and toast success. A failed
 * save records only a console line — no notice, server untouched (the
 * accepted OSS quirk).
 */
export function useSaveSmtpConfig() {
  const queryClient = useQueryClient();
  const client = getAppClient();
  return useMutation({
    mutationFn: (patch: Record<string, string>) => updateInstanceConfigurations(client, patch),
    onSuccess: (rows) => {
      queryClient.setQueryData<InstanceConfigurationList | undefined>(adminKeys.configurations, (current) =>
        mergeConfigurationRows(current, rows)
      );
      notifySuccess(SMTP_SAVE_TOAST_TITLE, SMTP_SAVE_TOAST_MESSAGE);
    },
    onError: (error) => {
      console.error(SMTP_SAVE_ERROR_LOG, error);
    },
  });
}

/** Disable-success notice (ADM-018; wording pinned by the parity scenario). */
export const EMAIL_DISABLED_TOAST_TITLE = "Email feature disabled";
export const EMAIL_DISABLED_TOAST_MESSAGE = "Email feature has been disabled";
/** Disable-failure notice (ADM-018, NEWFRONT-476 intent: fresh copy). */
export const EMAIL_DISABLE_FAILED_TITLE = "Could not disable email";
export const EMAIL_DISABLE_FAILED_MESSAGE = "The email settings were left unchanged. Try again.";
/** Disable-failure console line (ADM-018; pinned by the parity scenario). */
export const EMAIL_DISABLE_ERROR_LOG = "Error disabling the email";

/**
 * Turn the master email toggle off (ADM-018): DELETE the feature with the
 * local mail values blanked optimistically (the master flag to "0", the
 * security pair kept), restored if the call fails. Success toasts; failure
 * toasts the error and leaves the toggle on with the form visible
 * (NEWFRONT-476 intent), so the display matches the server.
 */
export function useDisableEmailFeature() {
  const queryClient = useQueryClient();
  const client = getAppClient();
  return useMutation({
    mutationFn: () => disableEmailFeature(client),
    onMutate: async () => {
      await queryClient.cancelQueries({ queryKey: adminKeys.configurations });
      const previous = queryClient.getQueryData<InstanceConfigurationList>(adminKeys.configurations);
      queryClient.setQueryData<InstanceConfigurationList>(adminKeys.configurations, (current) =>
        (current ?? []).map((row) =>
          BLANKED_MAIL_KEYS.has(row.key) ? { ...row, value: row.key === SMTP_ENABLED_KEY ? "0" : "" } : row
        )
      );
      return { previous };
    },
    onError: (error, _variables, context) => {
      if (context?.previous !== undefined) {
        queryClient.setQueryData(adminKeys.configurations, context.previous);
      }
      console.error(EMAIL_DISABLE_ERROR_LOG, error);
      notifyError(EMAIL_DISABLE_FAILED_TITLE, EMAIL_DISABLE_FAILED_MESSAGE);
    },
    onSuccess: () => {
      notifySuccess(EMAIL_DISABLED_TOAST_TITLE, EMAIL_DISABLED_TOAST_MESSAGE);
    },
  });
}

/**
 * Send the test email (ADM-021): POST the receiver address and report the
 * outcome. The check grades the stored configuration, so callers open it
 * only from a valid, saved form (NEWFRONT-477 save-first interim).
 */
export function useCheckEmailCredentials() {
  const client = getAppClient();
  return useMutation({
    mutationFn: (receiverEmail: string) => checkEmailCredentials(client, receiverEmail),
  });
}
