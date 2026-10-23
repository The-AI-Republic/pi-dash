// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// God-mode entry route (ADM-001, from parity/specs/admin.md §1.1; the
// cloud edition swaps the screen per §10.1): the OSS bootstrap gate by
// default, or the edition's cloud entry resolver when one is slotted.
// Redirect prefill and error params pass through verbatim; the gate
// resolves the branch and the forms read them.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { edition } from "../../core/edition/index.js";
import { asString } from "../../core/session/index.js";
import { AdminEntryGate, AdminHomeGuard, type AdminEntrySearch } from "../../features/admin/index.js";

/**
 * The router's default search parser JSON-parses each value, so the
 * numeric admin codes (`5180`, `5150`, ...) arrive as numbers and were
 * dropped by the string guard — every numeric rejection then came back
 * silent (NEWFRONT-457). Wire values stay verbatim: numbers and
 * booleans are read by their URL spelling, exactly as the server wrote
 * them.
 */
function asSearchString(raw: unknown): string | undefined {
  if (typeof raw === "number" && Number.isFinite(raw)) return String(raw);
  if (typeof raw === "boolean") return raw ? "true" : "false";
  return asString(raw);
}

/** Search schema: unknown or empty params fall back to absent. */
export function parseAdminEntrySearch(raw: Record<string, unknown>): AdminEntrySearch {
  const first_name = asSearchString(raw["first_name"]);
  const last_name = asSearchString(raw["last_name"]);
  const company = asSearchString(raw["company"]);
  const company_name = asSearchString(raw["company_name"]);
  const email = asSearchString(raw["email"]);
  const is_telemetry_enabled = asSearchString(raw["is_telemetry_enabled"]);
  const error_code = asSearchString(raw["error_code"]);
  const error_message = asSearchString(raw["error_message"]);
  return {
    ...(first_name === undefined ? {} : { first_name }),
    ...(last_name === undefined ? {} : { last_name }),
    ...(company === undefined ? {} : { company }),
    ...(company_name === undefined ? {} : { company_name }),
    ...(email === undefined ? {} : { email }),
    ...(is_telemetry_enabled === undefined ? {} : { is_telemetry_enabled }),
    ...(error_code === undefined ? {} : { error_code }),
    ...(error_message === undefined ? {} : { error_message }),
  };
}

export const Route = createFileRoute("/god-mode/")({
  validateSearch: parseAdminEntrySearch,
  component: GodModeEntryComponent,
});

function GodModeEntryComponent(): React.ReactElement {
  const search = Route.useSearch();
  // The cloud edition fills the slot with its OIDC resolver (ADM-074);
  // OSS leaves it absent and renders the bootstrap gate. Either way the
  // home guard (ADM-009) replace-navigates a signed-in admin to general
  // settings and shows the loader while the check resolves.
  const CloudEntry = edition.slots?.godModeCloudEntry;
  return (
    <AdminHomeGuard>{CloudEntry === undefined ? <AdminEntryGate search={search} /> : <CloudEntry />}</AdminHomeGuard>
  );
}
