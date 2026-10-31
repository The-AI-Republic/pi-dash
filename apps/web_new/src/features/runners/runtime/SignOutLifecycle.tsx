// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Root-mounted desktop sign-out lifecycle (RUN-047). Registration happens
// when the app root loads, before any route-specific runner chunk, so every
// sign-out surface shares the same confirmation and runtime disposal path.
// Only the prompt bridge is eager: the confirmation dialog, its kit
// controls and the runtime service load on the first desktop sign-out, so
// they never sit in the initial route chain.

import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform, type AgentRuntimeApi } from "../../../core/platform/index.js";
import { registerRuntimeSignOutHooks } from "../../../core/session/index.js";
import {
  attachSignOutPromptHost,
  beginSignOutPromptLoad,
  confirmRuntimeSignOut,
  detachSignOutPromptHost,
  failSignOutPromptLoad,
} from "./signOutPrompt.js";

const runtime = platform.agentRuntime;

interface SignOutDialogModule {
  RuntimeSignOutDialog: React.ComponentType;
}

let dialogModule: Promise<SignOutDialogModule> | null = null;

function loadSignOutDialog(): Promise<SignOutDialogModule> {
  beginSignOutPromptLoad();
  dialogModule ??= import("./SignOutDialog.js").catch((error: unknown) => {
    // A chunk that cannot load means nothing can ask the user; pending and
    // later confirmations keep the history rather than trapping them here.
    failSignOutPromptLoad();
    throw error;
  });
  return dialogModule;
}

/**
 * Open the confirmation. The fetch starts when the hooks register, and a
 * request that arrives mid-fetch waits for the dialog to mount, so a sign-out
 * never skips confirmation just because the chunk was slow.
 */
async function promptWithDialog(): Promise<Awaited<ReturnType<typeof confirmRuntimeSignOut>>> {
  try {
    await loadSignOutDialog();
  } catch {
    // The bridge has marked the dialog unavailable and settled anything
    // queued; asking now takes the same keep-history default.
  }
  return confirmRuntimeSignOut();
}

/** Register the desktop hooks; missing web capability is deliberately inert. */
export function registerRuntimeSignOutLifecycle(runtimeApi: AgentRuntimeApi | null | undefined): void {
  if (!runtimeApi) return;
  beginSignOutPromptLoad();
  registerRuntimeSignOutHooks({
    confirmRuntimeSignOut: () => promptWithDialog(),
    disposeRuntimeForSignOut: async (choice) => {
      // Disposal follows a confirmed desktop sign-out only, so keep the
      // larger runtime service out of the initial route chain too.
      const { disposeRuntimeForSignOut } = await import("./service.js");
      await disposeRuntimeForSignOut({ choice, client: getAppClient(), runtime: runtimeApi });
    },
  });
}

registerRuntimeSignOutLifecycle(runtime);

/**
 * Confirmation host mounted above every route. On desktop it loads the
 * dialog chunk once and renders it, keeping one prompt registration alive
 * for the life of the app; web renders nothing and registers no hooks.
 */
export function RuntimeSignOutLifecycle(): React.ReactElement | null {
  const [Dialog, setDialog] = React.useState<React.ComponentType | null>(null);

  React.useEffect(() => {
    if (!runtime) return;
    attachSignOutPromptHost();
    let cancelled = false;
    void loadSignOutDialog().then(
      (loaded) => {
        if (!cancelled) setDialog(() => loaded.RuntimeSignOutDialog);
      },
      () => {
        // Without the dialog chunk, confirmations keep their history default.
      }
    );
    return () => {
      cancelled = true;
      detachSignOutPromptHost();
    };
  }, []);

  return Dialog ? <Dialog /> : null;
}
