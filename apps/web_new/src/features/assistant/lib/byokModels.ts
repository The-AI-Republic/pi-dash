// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// BYOK model-ID list rules (NEWPIDASH-394, design sections 4.4, 7.2 and
// 7.3). Pure on purpose: the server's list shape, one ID's display
// status, and the "Save anyway" eligibility are unit-testable without a
// DOM or a server.
import type { ModelTestResults } from "@pidash/api-client";

/** One model ID's display status in the editor. */
export type ModelIdStatus = "verified" | "unverified" | "failed";

/**
 * The server's list rule (design section 4.4): trim, drop empty entries
 * and duplicates, keep the typed order.
 */
export function normalizeModelIds(ids: readonly string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of ids) {
    const id = raw.trim();
    if (id === "" || seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}

/**
 * One ID's display status: a passing stamp wins, then a message from the
 * last refused write or test, otherwise the ID is simply unverified.
 */
export function modelIdStatus(
  model: string,
  verifiedAt: Readonly<Record<string, string | null | undefined>>,
  failures: Readonly<Record<string, string>>
): ModelIdStatus {
  const stamp = verifiedAt[model];
  if (stamp !== undefined && stamp !== null) return "verified";
  if (failures[model] !== undefined) return "failed";
  return "unverified";
}

/**
 * The provider message per model from a refused write or a test answer:
 * failed and unverified entries both carry one (design section 6.3).
 * Verified entries contribute nothing.
 */
export function modelFailureDetails(results: ModelTestResults): Record<string, string> {
  const out: Record<string, string> = {};
  for (const entry of results.models) {
    if (entry.status === "verified") continue;
    out[entry.model] = entry.detail ?? entry.code ?? "";
  }
  return out;
}

/**
 * The IDs a "Save anyway" retry may persist: the refused write's
 * unverified IDs, and only when no ID failed — failed IDs must be fixed
 * or removed first (design section 7.2). Null when there is nothing a
 * retry could save.
 */
export function saveAnywayModels(results: ModelTestResults): string[] | null {
  if (results.models.some((entry) => entry.status === "failed")) return null;
  const unverified = results.models.filter((entry) => entry.status === "unverified").map((entry) => entry.model);
  return unverified.length === 0 ? null : unverified;
}
