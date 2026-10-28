// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Development-only workflow rules for the ISS-233 cloud leg
// (parity/specs/issues.md §23, NEWFRONT-984). No rule source exists yet —
// there is no workflow model, endpoint, or settings surface in either
// repo — so the cloud bootstrap cannot register real rules. This module
// is the provisional stand-in: in development builds only, the refusal
// consults one session-storage key the parity run sets before load; the
// production bundle never reads it (the DEV branch compiles out) and a
// missing or malformed key means no rules, never a refusal. NEWFRONT-994
// (workflow rules source) replaces this provider with the real
// configuration and rewires the cloud leg onto it; until then ISS-233
// stays out of new-green.
import type { WorkflowRefusal, WorkflowRules } from "./providers.js";

/** Session key the parity run writes before load (dev builds only). */
export const PARITY_WORKFLOW_RULES_KEY = "pidash.parity.workflowRules.v1";

/** One injected rule: the denied hop plus the sentence the surface shows. */
export interface ParityWorkflowRule {
  /** Origin state, or null for creation into the target. */
  from: string | null;
  to: string;
  reason: string;
}

function isRule(value: unknown): value is ParityWorkflowRule {
  if (typeof value !== "object" || value === null) return false;
  const row = value as Record<string, unknown>;
  return (
    (typeof row["from"] === "string" || row["from"] === null) &&
    typeof row["to"] === "string" &&
    (row["to"] as string) !== "" &&
    typeof row["reason"] === "string" &&
    (row["reason"] as string) !== ""
  );
}

/**
 * The injected rules, or null outside development builds, without the
 * key, or when the key fails to parse. Never throws: a broken key
 * reads as no rules so a bad write cannot wedge a dev session. The
 * build flag rides a defaulted parameter so tests can exercise the
 * production branch; production callers never pass it.
 */
export function readParityWorkflowRules(devMode: boolean = import.meta.env.DEV): ParityWorkflowRule[] | null {
  if (devMode !== true) return null;
  let raw: string | null = null;
  try {
    raw = window.sessionStorage.getItem(PARITY_WORKFLOW_RULES_KEY);
  } catch {
    return null;
  }
  if (raw === null) return null;
  let parsed: unknown = null;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch {
    return null;
  }
  if (!Array.isArray(parsed) || !parsed.every(isRule)) return null;
  return parsed;
}

function refusalFor(fromStateId: string | null, toStateId: string): WorkflowRefusal | null {
  const rules = readParityWorkflowRules();
  const hit = rules?.find((rule) => rule.from === fromStateId && rule.to === toStateId);
  return hit === undefined ? null : { reason: hit.reason };
}

/**
 * The provisional cloud rules: each hop is refused exactly when an
 * injected rule names it. Purpose-agnostic on purpose — the injected
 * shape names hops, not surfaces — since the leg proves the gate
 * mechanism, not a configuration surface that does not exist yet.
 */
export function parityWorkflowRulesProvider(): WorkflowRules {
  return {
    refusal: (input) => refusalFor(input.fromStateId, input.toStateId),
    creationRefusal: (input) => refusalFor(null, input.columnKey),
  };
}
