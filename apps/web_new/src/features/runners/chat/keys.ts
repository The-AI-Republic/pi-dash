// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner-chat cache keys (NEWFRONT-549). Every key hangs inside the runners
// subtree so a runner delete, a workspace switch or sign-out drops the chat
// history with the rest of the area. A conversation belongs to a runner,
// not to the route that opened it, so the session key carries the runner.

import { runnersKeys } from "../api/keys.js";

export const chatKeys = {
  all: (workspaceSlug: string) => [...runnersKeys.all(workspaceSlug), "chat"] as const,
  runner: (workspaceSlug: string, runnerId: string) => [...chatKeys.all(workspaceSlug), "runner", runnerId] as const,
  sessions: (workspaceSlug: string, runnerId: string) =>
    [...chatKeys.all(workspaceSlug), "sessions", runnerId] as const,
  messages: (workspaceSlug: string, sessionId: string) =>
    [...chatKeys.all(workspaceSlug), "messages", sessionId] as const,
  workingDir: (workspaceSlug: string, workspaceId: string, runnerId: string) =>
    [...chatKeys.all(workspaceSlug), "working-dir", workspaceId, runnerId] as const,
};
