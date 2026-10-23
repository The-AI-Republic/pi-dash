// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project facts the modules list reads (parity/specs/modules.md §2.1,
// §2.2, MOD-001/002). The list needs three things off the project record:
// the name for the breadcrumb and tab title, the modules switch that
// decides the gate, and the project zone the date helpers use. It reads
// them through this feature's own narrow projection rather than the
// shell's full project row, because a server that omits `module_view`
// must still parse here: an absent switch is an off switch (MOD-002),
// not a contract failure that would leave the list on its skeleton.

import { queryOptions } from "@tanstack/react-query";
import * as z from "zod/mini";
import type { ApiClient } from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { moduleKeys } from "./keys.js";

/**
 * The projection the modules area reads. Unknown fields pass through
 * untouched. The switch and the zone are read as unknown values and
 * narrowed by the helpers below, so an absent flag, a null or a value of
 * the wrong type all still yield a usable name: only a literal `true`
 * turns the list on, and only a named zone feeds the date helpers.
 */
export const ModuleProjectFacts = z.looseObject({
  name: z.string(),
  module_view: z.optional(z.unknown()),
  timezone: z.optional(z.unknown()),
});
export type ModuleProjectFacts = z.infer<typeof ModuleProjectFacts>;

export function moduleProjectPath(workspaceSlug: string, projectId: string): string {
  return `/api/workspaces/${workspaceSlug}/projects/${projectId}/`;
}

/** GET one project row, parsed through the modules projection. */
export async function getModuleProjectFacts(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string
): Promise<ModuleProjectFacts> {
  const path = moduleProjectPath(workspaceSlug, projectId);
  const body = await client.getJson<unknown>(path);
  return client.parse(ModuleProjectFacts, body, `GET ${path}`);
}

/**
 * Whether the project's modules switch is on. Only a literal `true`
 * enables the list: an absent flag, a null, a wrong-typed value, or a
 * project record that has not arrived all read as off (MOD-002).
 */
export function moduleViewEnabled(flag: unknown): boolean {
  return flag === true;
}

/** The project's zone, or null when the record carries no usable name. */
export function moduleProjectTimeZone(facts: ModuleProjectFacts | undefined): string | null {
  const zone = facts?.timezone;
  if (typeof zone !== "string" || zone.trim() === "") return null;
  return zone;
}

/**
 * One project row for the modules list. Same freshness as other project
 * reference reads; no focus refetch — the area is fetch-on-mount.
 */
export function moduleProjectFactsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: moduleKeys.projectFacts(workspaceSlug, projectId),
    queryFn: () => getModuleProjectFacts(client, workspaceSlug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}
