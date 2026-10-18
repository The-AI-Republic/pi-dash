// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Root-mounted desktop sign-out lifecycle (RUN-047). Registration happens
// when the app root loads, before any route-specific runner chunk, so every
// sign-out surface shares the same confirmation and runtime disposal path.

import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform, type AgentRuntimeApi } from "../../../core/platform/index.js";
import { registerRuntimeSignOutHooks } from "../../../core/session/index.js";
import { confirmRuntimeSignOut, RuntimeSignOutDialog } from "./SignOutDialog.js";

const runtime = platform.agentRuntime;

/** Register the desktop hooks; missing web capability is deliberately inert. */
export function registerRuntimeSignOutLifecycle(runtimeApi: AgentRuntimeApi | null | undefined): void {
  if (!runtimeApi) return;
  registerRuntimeSignOutHooks({
    confirmRuntimeSignOut: () => confirmRuntimeSignOut(),
    disposeRuntimeForSignOut: async (choice) => {
      // Disposal is needed only after a desktop user confirms sign-out, so
      // keep the larger runtime service out of the initial route chain.
      const { disposeRuntimeForSignOut } = await import("./service.js");
      await disposeRuntimeForSignOut({ choice, client: getAppClient(), runtime: runtimeApi });
    },
  });
}

registerRuntimeSignOutLifecycle(runtime);

/** Mount the desktop confirmation once above every route; web stays inert. */
export function RuntimeSignOutLifecycle(): React.ReactElement | null {
  return runtime ? <RuntimeSignOutDialog /> : null;
}
