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

/** Search schema: unknown or empty params fall back to absent. */
export function parseAdminEntrySearch(raw: Record<string, unknown>): AdminEntrySearch {
  const first_name = asString(raw["first_name"]);
  const last_name = asString(raw["last_name"]);
  const company = asString(raw["company"]);
  const company_name = asString(raw["company_name"]);
  const email = asString(raw["email"]);
  const is_telemetry_enabled = asString(raw["is_telemetry_enabled"]);
  const error_code = asString(raw["error_code"]);
  const error_message = asString(raw["error_message"]);
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
