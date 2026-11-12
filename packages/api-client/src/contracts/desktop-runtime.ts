// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop managed-agent contracts: whether the bundled engine may
// run here, and the machine enrollment that registers this desktop
// with a workspace. The engine profile and credential themselves are
// contracted in assistant-desktop.js (single copy); this file only
// reads the profile for the availability answer. Enrollment is
// desktop-only: a browser session is refused, so the live suite
// proves the refusal while stub tests prove the success shapes.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { getAgentProfile } from "./assistant-desktop.js";

export interface AgentAvailability {
  available: boolean;
  reasonCode: string;
}

/** Client-side marker for a profile read that never answered. */
export const AGENT_AVAILABILITY_UNKNOWN = "agent_profile_unreachable";

/**
 * Whether this server lets the bundled agent run, and why not when
 * it won't. Never throws: an unreachable API or an expired session
 * reads as unavailable with the client-side marker, which is what
 * the chat contact list shows while the server cannot be reached.
 * Reason copy stays with the slices; only the code travels here.
 */
export async function checkAgentAvailability(client: ApiClient): Promise<AgentAvailability> {
  try {
    const profile = await getAgentProfile(client);
    return { available: profile.available, reasonCode: profile.available ? "" : profile.reason_code };
  } catch {
    return { available: false, reasonCode: AGENT_AVAILABILITY_UNKNOWN };
  }
}

/**
 * Answer to a desktop enrollment. `machine_token` is shown once and
 * is written to the daemon's token slot exactly like a hand-installed
 * runner's; `dev_machine_id` names the provisioned machine row.
 * Enrolling again for the same machine and workspace rotates the
 * token onto the same row rather than accumulating rows.
 */
export const DesktopEnrollResult = z.object({
  dev_machine_id: z.uuid(),
  machine_token: z.string(),
  workspace_slug: z.string(),
  managed_runner_enabled: z.boolean(),
  graceful_stop_seconds: z.number(),
});
export type DesktopEnrollResult = z.infer<typeof DesktopEnrollResult>;

export interface DesktopEnrollInput {
  workspaceSlug: string;
  hostLabel: string;
  /** Desktop build version; below the server floor the call is refused. */
  appVersion?: string;
}

/**
 * Enroll this desktop machine against a workspace. Both names are
 * required; an unknown or foreign workspace reads as not found
 * without saying which, and a too-old desktop is told to upgrade.
 */
export async function createDesktopEnroll(client: ApiClient, input: DesktopEnrollInput): Promise<DesktopEnrollResult> {
  const path = "/api/v1/runner/dev-machines/desktop-enroll/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      workspace_slug: input.workspaceSlug,
      host_label: input.hostLabel,
      ...(input.appVersion === undefined ? {} : { app_version: input.appVersion }),
    },
  });
  return client.parse(DesktopEnrollResult, envelope.data, `POST ${path}`);
}

/**
 * Revoke this desktop's enrollments. Without a host label every
 * bundled machine owned by the caller is revoked; with one, only
 * that host. Always answers 204, even when nothing matched, and the
 * runner rows survive (marked offline) so the next sign-in reuses
 * them. Sign-out treats a failure here as a warning, never a trap.
 */
export async function deleteDesktopEnroll(client: ApiClient, hostLabel?: string): Promise<void> {
  const path = "/api/v1/runner/dev-machines/desktop-enroll/";
  await client.delete(path, { query: { host_label: hostLabel } });
}
