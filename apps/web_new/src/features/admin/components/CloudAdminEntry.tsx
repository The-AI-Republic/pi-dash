// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud god-mode entry resolver (ADM-074, from parity/specs/admin.md
// §10): authorization over OIDC behind the same /god-mode/ path the OSS
// bootstrap gate serves. The admin check plus a one-shot probe resolve
// to exactly one outcome — the brand spinner (authorized admin,
// bouncing anonymous) while navigation lands, the terminal denied
// screen (signed-in non-admin), or the retryable error screen
// (transient failure) — with no password field and no setup form on
// any branch. The cloud edition mounts this through the
// godModeCloudEntry slot; the home guard wraps it like the OSS gate.
import * as React from "react";

import { cloudEntryReturnTo, decideCloudEntry, toCloudEntryProbe, toCloudEntrySession } from "../cloudEntry.js";
import { useAdminSession, useCloudAuthProbe, useCloudEntryRetry, useCloudSignInUrl } from "../hooks.js";
import { AdminBrandLoader } from "./AdminBrandLoader.js";
import { CloudAccessDeniedScreen } from "./CloudAccessDeniedScreen.js";
import { CloudEntryErrorScreen } from "./CloudEntryErrorScreen.js";

export function CloudAdminEntry(): React.ReactElement {
  const sessionQuery = useAdminSession();
  const session = toCloudEntrySession(sessionQuery);
  const probeQuery = useCloudAuthProbe(session);
  const probe = toCloudEntryProbe(probeQuery);
  const outcome = decideCloudEntry(session, probe);
  const signInUrl = useCloudSignInUrl(
    cloudEntryReturnTo(globalThis.window.location.pathname, globalThis.window.location.search)
  );
  const retry = useCloudEntryRetry();
  React.useEffect(() => {
    if (outcome === "bounce") {
      globalThis.window.location.replace(signInUrl);
    }
  }, [outcome, signInUrl]);
  if (outcome === "denied") return <CloudAccessDeniedScreen />;
  if (outcome === "retry") return <CloudEntryErrorScreen onRetry={retry} />;
  return <AdminBrandLoader />;
}
