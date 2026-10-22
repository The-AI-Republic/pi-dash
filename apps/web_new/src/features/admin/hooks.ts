// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin data hooks (Data layer page). Components read server data and
// flip the loop toggles through these, never through raw client calls.

import { mutationOptions, useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import {
  checkEmailCredentials,
  checkInstanceWorkspaceSlug,
  cloudSignInUrl,
  createInstanceWorkspace,
  createLoopJob,
  deleteLoopJob,
  disableEmailFeature,
  updateInstance,
  updateInstanceConfigurations,
  updateLoopJob,
  type ApiClient,
  type InstanceBootstrap,
  type InstanceConfigurationList,
  type InstancePatch,
  type InstanceWorkspaceCreateInput,
  type LoopJobCreateInput,
  type LoopJobList,
  type LoopJobPatchInput,
} from "@pidash/api-client";
import { showToast } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../core/query/optimistic.js";
import { getToastManager, notifyError, notifySuccess } from "../../shared/shell/index.js";
import type { CloudEntrySessionState } from "./cloudEntry.js";
import { BLANKED_MAIL_KEYS, SMTP_ENABLED_KEY, type ConfigValueMap } from "./emailConfig.js";
import { LOOP_TOGGLE_FAILURE_MESSAGE, LOOP_TOGGLE_FAILURE_TITLE } from "./loopJobs.js";
import { LOOP_JOB_DELETE_FAILED_MESSAGE, LOOP_JOB_DELETE_FAILED_TITLE } from "./loopJobDetail.js";
import { LOOP_JOB_SAVE_FAILED_TITLE, resolveLoopJobSaveError } from "./loopJobModal.js";
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
  instanceRunnersQueryOptions,
  instanceWorkspacesQueryOptions,
  loopJobDetailQueryOptions,
  loopJobsQueryOptions,
  loopJobTargetsQueryOptions,
  mergeConfigurationRows,
} from "./queries.js";
import {
  CONFIG_SAVE_FAILURE_MESSAGE,
  CONFIG_SAVE_FAILURE_TITLE,
  CONFIG_SAVE_PROGRESS_TITLE,
  CONFIG_SAVE_SUCCESS_MESSAGE,
  CONFIG_SAVE_SUCCESS_TITLE,
  DISABLE_WORKSPACE_CREATION_KEY,
  encodeWorkspaceCreationRestricted,
} from "./workspace.js";
import {
  isRestrictedWorkspaceSlug,
  WORKSPACE_CREATE_FAILURE_TITLE,
  WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_TITLE,
  WorkspaceSlugProbeError,
  WorkspaceSlugTakenError,
} from "./workspaceCreate.js";

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
 * into it or refresh it through explicit invalidation, so pages
 * always render stored state.
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
 * The instance workspace list (ADM-038, ADM-040): cursor-paginated, ten
 * per page. `fetchNextPage` appends; nothing shown is refetched.
 */
export function useInstanceWorkspaces() {
  return useInfiniteQuery(instanceWorkspacesQueryOptions(getAppClient()));
}

/**
 * Flip the workspace-creation restriction (ADM-037): a single-key
 * configuration write with a progress notice resolving to success or
 * failure. Success folds the server answer into the shared cache so
 * the switch (and every settings page) re-reads the stored value; a
 * failed save keeps the stored value and the switch re-enables.
 */
export function useRestrictWorkspaceCreation() {
  const queryClient = useQueryClient();
  return useMutation<InstanceConfigurationList, Error, boolean>({
    mutationFn: async (restricted) => {
      const manager = getToastManager();
      const progressId = showToast(manager, { title: CONFIG_SAVE_PROGRESS_TITLE, timeout: 0 });
      try {
        const rows = await updateInstanceConfigurations(getAppClient(), {
          [DISABLE_WORKSPACE_CREATION_KEY]: encodeWorkspaceCreationRestricted(restricted),
        });
        manager.close(progressId);
        notifySuccess(CONFIG_SAVE_SUCCESS_TITLE, CONFIG_SAVE_SUCCESS_MESSAGE);
        return rows;
      } catch (error) {
        manager.close(progressId);
        notifyError(CONFIG_SAVE_FAILURE_TITLE, CONFIG_SAVE_FAILURE_MESSAGE);
        throw error;
      }
    },
    onSuccess: (rows) => {
      queryClient.setQueryData<InstanceConfigurationList | undefined>(adminKeys.configurations, (cached) =>
        mergeConfigurationRows(cached, rows)
      );
    },
  });
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

export function createLoopJobMutationOptions(client: ApiClient, queryClient: QueryClient) {
  return mutationOptions({
    mutationFn: (input: LoopJobCreateInput) => createLoopJob(client, input),
    onError: (error: unknown) => {
      notifyError(LOOP_JOB_SAVE_FAILED_TITLE, resolveLoopJobSaveError(error));
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: adminKeys.loopJobs });
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

/**
 * Saves the create modal (ADM-055). Silent on success — the modal
 * closes and the caller re-reads the revalidated list; a failed save
 * raises the server-text notice and keeps the modal open.
 */
export function useCreateLoopJob() {
  return useMutation(createLoopJobMutationOptions(getAppClient(), useQueryClient()));
}

export interface LoopJobUpdate {
  id: string;
  patch: LoopJobPatchInput;
}

export function updateLoopJobMutationOptions(client: ApiClient, queryClient: QueryClient) {
  return mutationOptions({
    mutationFn: (update: LoopJobUpdate) => updateLoopJob(client, update.id, update.patch),
    onError: (error: unknown) => {
      notifyError(LOOP_JOB_SAVE_FAILED_TITLE, resolveLoopJobSaveError(error));
    },
    onSettled: (_data: unknown, _error: unknown, update: LoopJobUpdate) => {
      void queryClient.invalidateQueries({ queryKey: adminKeys.loopJobs });
      void queryClient.invalidateQueries({ queryKey: adminKeys.loopJobDetail(update.id) });
    },
  });
}

/**
 * Saves the edit modal (ADM-056). The list and the edited job both
 * revalidate on settle, so the list and the detail converge; failures
 * behave like the create save.
 */
export function useUpdateLoopJob() {
  return useMutation(updateLoopJobMutationOptions(getAppClient(), useQueryClient()));
}

/** One loop job with its stats rollup for the detail page (ADM-058). */
export function useLoopJobDetail(jobId: string) {
  return useQuery(loopJobDetailQueryOptions(getAppClient(), jobId));
}

/**
 * One job's targets for the detail table (ADM-058/059). Each skip
 * filter value reads its own cached entry; resetting to all reuses
 * the cache with no refetch.
 */
export function useLoopJobTargets(jobId: string, skipReason: string) {
  return useQuery(loopJobTargetsQueryOptions(getAppClient(), jobId, skipReason));
}

export function deleteLoopJobMutationOptions(client: ApiClient, queryClient: QueryClient) {
  return mutationOptions({
    mutationFn: (jobId: string) => deleteLoopJob(client, jobId),
    onError: () => {
      notifyError(LOOP_JOB_DELETE_FAILED_TITLE, LOOP_JOB_DELETE_FAILED_MESSAGE);
    },
    onSettled: (_data: unknown, _error: unknown, jobId: string) => {
      void queryClient.invalidateQueries({ queryKey: adminKeys.loopJobs });
      void queryClient.invalidateQueries({ queryKey: adminKeys.loopJobDetail(jobId) });
    },
  });
}

/**
 * Deletes the job immediately with no confirmation (ADM-060/073).
 * Silent on success — the page toasts and returns to the list; a
 * failed delete raises the fixed error notice and stays. The list
 * and the deleted job revalidate on settle either way.
 */
export function useDeleteLoopJob() {
  return useMutation(deleteLoopJobMutationOptions(getAppClient(), useQueryClient()));
}

export function createInstanceWorkspaceMutationOptions(client: ApiClient, queryClient: QueryClient) {
  return mutationOptions({
    mutationFn: async (input: InstanceWorkspaceCreateInput) => {
      let available: boolean;
      try {
        available = (await checkInstanceWorkspaceSlug(client, input.slug)).status;
      } catch {
        throw new WorkspaceSlugProbeError(input.slug);
      }
      if (!available || isRestrictedWorkspaceSlug(input.slug)) {
        throw new WorkspaceSlugTakenError(input.slug);
      }
      return createInstanceWorkspace(client, input);
    },
    onError: (error: unknown) => {
      if (error instanceof WorkspaceSlugTakenError) return;
      if (error instanceof WorkspaceSlugProbeError) {
        notifyError(WORKSPACE_CREATE_FAILURE_TITLE, WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE);
      } else {
        notifyError(WORKSPACE_CREATE_FAILURE_TITLE, WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE);
      }
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: adminKeys.workspaces });
      notifySuccess(WORKSPACE_CREATE_SUCCESS_TITLE, WORKSPACE_CREATE_SUCCESS_MESSAGE);
    },
  });
}

/**
 * Creates the workspace behind the create form (ADM-044/045): the slug
 * probe runs first and a taken or restricted slug raises the taken
 * signal the form renders inline, with no create POST. A failed probe
 * and a failed create both toast the error and stay. Success toasts
 * and revalidates the list; the form navigates home on its own
 * success handler.
 */
export function useCreateInstanceWorkspace() {
  return useMutation(createInstanceWorkspaceMutationOptions(getAppClient(), useQueryClient()));
}

/**
 * One runners page for the runners table (ADM-080). Each filter value
 * and page reads its own cached entry; the page restarts from 1 when
 * the filter changes, which the query key carries as its own entry.
 */
export function useInstanceRunners(provisioning: string, page: number) {
  return useQuery(instanceRunnersQueryOptions(getAppClient(), provisioning, page));
}
