// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop chat bootstrap (NEWFRONT-553, RUN-033-036). Installs the local
// engine's transport in place of the cloud one the first time the runtime
// resumes for a signed account. Idempotent, and inert without the native
// local-chat capability, so the web and cloud bundles never take this path.

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { getRuntimeAccount, ensureChatRuntime } from "../runtime/service.js";
import { storedApprovalMode } from "./approvalCopy.js";
import { createLocalChatTransport } from "./localTransport.js";
import { setChatTransport } from "./transport.js";

let installed = false;

/** Workspace slug is the first path segment of every in-app route. */
export function workspaceSlugFromPath(pathname: string): string {
  return decodeURIComponent(pathname.split("/")[1] ?? "");
}

/**
 * Install the local chat transport once. Safe to call on every runtime
 * resume: after the first call it does nothing.
 */
export function registerLocalChatTransport(): void {
  const api = platform.localChat;
  if (api === undefined || installed) return;
  installed = true;
  setChatTransport(
    createLocalChatTransport({
      api,
      account: getRuntimeAccount,
      workspaceSlug: () => workspaceSlugFromPath(globalThis.location.pathname),
      storedMode: storedApprovalMode,
      ensureRuntime: () =>
        ensureChatRuntime({
          workspaceSlug: workspaceSlugFromPath(globalThis.location.pathname),
          client: getAppClient(),
          runtime: platform.agentRuntime,
        }),
    })
  );
}

/** Test seam: forget the installation and restore the cloud transport. */
export function resetLocalChatTransportForTests(): void {
  installed = false;
  setChatTransport(null);
}
