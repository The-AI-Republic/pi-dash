// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bootstrap gate (ADM-001, from parity/specs/admin.md §1.1): the root
// resolves to exactly one branch from the single instance-info read —
// the loader while unresolved, the failure screen on error, the
// setup form when setup is not done, or the sign-in form when it is.
// OSS-only: the cloud edition replaces this screen (ADM-074).

import * as React from "react";

import { useAppInstanceBootstrap } from "../../instance/index.js";
import { AdminBrandLoader } from "./AdminBrandLoader.js";
import { AdminSetupForm } from "./AdminSetupForm.js";
import { AdminSignInForm } from "./AdminSignInForm.js";
import { InstanceFailureScreen } from "./InstanceFailureScreen.js";

/** Redirect params the entry screens read, in URL spelling. */
export interface AdminEntrySearch {
  first_name?: string | undefined;
  last_name?: string | undefined;
  company?: string | undefined;
  company_name?: string | undefined;
  email?: string | undefined;
  is_telemetry_enabled?: string | undefined;
  error_code?: string | undefined;
  error_message?: string | undefined;
}

export function AdminEntryGate({ search }: { search: AdminEntrySearch }): React.ReactElement {
  // The startup read accepts the backend's unprovisioned envelope, so a
  // fresh instance reaches the setup form instead of the failure screen.
  const bootstrap = useAppInstanceBootstrap();
  const setupDone = bootstrap.data?.setupDone;
  if (setupDone !== undefined) {
    return setupDone ? <AdminSignInForm search={search} /> : <AdminSetupForm search={search} />;
  }
  if (bootstrap.isError) {
    return <InstanceFailureScreen />;
  }
  return <AdminBrandLoader />;
}
