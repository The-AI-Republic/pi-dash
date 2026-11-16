// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant layout (parity/specs/agents.md §9/§11). The conversation rail
// sits beside the routed content; the rail read is the only shared state
// between the landing and a conversation. On the desktop the rail is gone:
// the routed content is the local chat with its own history column
// (NEWPIDASH-300).

import { Outlet } from "@tanstack/react-router";
import * as React from "react";

import { hasLocalChat } from "../../../core/platform/index.js";
import { ConversationRail } from "./ConversationRail.js";

export interface AssistantLayoutProps {
  workspaceSlug: string;
}

export function AssistantLayout({ workspaceSlug }: AssistantLayoutProps): React.ReactElement {
  // On the desktop the routed content is the local chat, whose own
  // history column carries the conversations; the server rail would list
  // cloud threads the desktop page never opens (NEWPIDASH-300).
  const local = hasLocalChat();
  return (
    <div data-testid="assistant-layout" className="flex h-full w-full overflow-hidden">
      {local ? null : <ConversationRail key={workspaceSlug} workspaceSlug={workspaceSlug} />}
      <main className="min-w-0 flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
