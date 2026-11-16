// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Integrations reads and writes (parity/specs/profile.md §11, PROF-025).
// One status read backs every workspace the member administers; starting
// an install hands the browser to the host, and a manual refresh re-reads
// the installation that host reports. Failures keep the panel and report
// the server's own message.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getGithubAppStatus,
  refreshGithubAppInstallation,
  startGithubAppInstall,
  type GithubAppInstallation,
  type GithubAppStatus,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../core/query/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/toaster.js";
import {
  INSTALL_FAILED_FALLBACK,
  INSTALL_FAILED_TITLE,
  REFRESH_FAILED_FALLBACK,
  REFRESH_FAILED_TITLE,
  REFRESH_OK_MESSAGE,
  REFRESH_OK_TITLE,
} from "./lib/copy.js";

export const integrationsKeys = {
  all: ["profile-integrations"] as const,
  status: ["profile-integrations", "github-app"] as const,
};

/** The admin-scoped installation status; never polled (PROF-027). */
export function useGithubAppStatus() {
  return useQuery({
    queryKey: integrationsKeys.status,
    queryFn: () => getGithubAppStatus(getAppClient()),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

function failureMessage(error: unknown, fallback: string): string {
  const body = (error as { body?: unknown } | null)?.body;
  if (body !== null && typeof body === "object") {
    const record = body as Record<string, unknown>;
    for (const key of ["error", "detail"]) {
      const value = record[key];
      if (typeof value === "string" && value.trim() !== "") return value;
    }
  }
  return fallback;
}

/** Begin the host-side install; answers the address to hand off to. */
export function useStartGithubInstall() {
  return useMutation<string, Error, string>({
    mutationFn: async (workspaceSlug) => {
      const start = await startGithubAppInstall(getAppClient(), workspaceSlug);
      return start.install_url;
    },
    onError: (error) => {
      notifyError(INSTALL_FAILED_TITLE, failureMessage(error, INSTALL_FAILED_FALLBACK));
    },
  });
}

/** Replace one workspace's installation in a cached status payload. */
export function withRefreshedInstallation(
  status: GithubAppStatus | undefined,
  workspaceSlug: string,
  installation: GithubAppInstallation
): GithubAppStatus | undefined {
  if (status === undefined) return undefined;
  return {
    ...status,
    workspaces: status.workspaces.map((row) =>
      row.slug === workspaceSlug ? { ...row, github_app: installation } : row
    ),
  };
}

/** Re-check one installation and replace the cached status row. */
export function useRefreshGithubInstall() {
  const queryClient = useQueryClient();
  return useMutation<GithubAppInstallation, Error, string>({
    mutationFn: (workspaceSlug) => refreshGithubAppInstallation(getAppClient(), workspaceSlug),
    onSuccess: (installation, workspaceSlug) => {
      queryClient.setQueryData<GithubAppStatus>(integrationsKeys.status, (previous) =>
        withRefreshedInstallation(previous, workspaceSlug, installation)
      );
      notifySuccess(REFRESH_OK_TITLE, REFRESH_OK_MESSAGE);
    },
    onError: (error) => {
      notifyError(REFRESH_FAILED_TITLE, failureMessage(error, REFRESH_FAILED_FALLBACK));
    },
  });
}
