// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant layout (parity/specs/agents.md §9/§11). The conversation rail
// sits beside the routed content; the rail read is the only shared state
// between the landing and a conversation.

import { Outlet } from "@tanstack/react-router";
import * as React from "react";

import { ConversationRail } from "./ConversationRail.js";

export interface AssistantLayoutProps {
  workspaceSlug: string;
}

export function AssistantLayout({ workspaceSlug }: AssistantLayoutProps): React.ReactElement {
  return (
    <div className="flex h-full w-full overflow-hidden">
      <ConversationRail key={workspaceSlug} workspaceSlug={workspaceSlug} />
      <main className="min-w-0 flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
