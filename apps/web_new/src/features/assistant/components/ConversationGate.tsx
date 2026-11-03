// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation shell (parity/specs/agents.md §9/§10, AGT-039/AGT-040 and
// the AGT-038 handoff). Resolves the provider configuration once (by
// credential lane, CLOUD-037) and hands the whole conversation body to the
// thread view, which owns the transcript, the composer and the one-shot
// first message. A keyless user gets the setup card in the body and the
// locked composer with the reminder.

import * as React from "react";

import { Spinner } from "@pidash/kit";

import { useAssistantConfig } from "../api/queries.js";
import { needsLaneSetup } from "../lib/gate.js";
import { ThreadView } from "./ThreadView.js";

export interface ConversationGateProps {
  workspaceSlug: string;
  threadId: string;
  /** Router state carried by the navigation that opened this conversation. */
  state: unknown;
}

export function ConversationGate({ workspaceSlug, threadId, state }: ConversationGateProps): React.ReactElement {
  const config = useAssistantConfig(workspaceSlug);
  const setup = needsLaneSetup(config.data);

  return (
    <div
      data-testid="assistant-thread"
      data-thread-id={threadId}
      data-settled={config.isPending ? "false" : "true"}
      className="h-full min-h-0"
    >
      {config.isPending ? (
        <div className="flex h-full items-center justify-center">
          <Spinner label="Loading assistant" />
        </div>
      ) : (
        <ThreadView
          key={threadId}
          workspaceSlug={workspaceSlug}
          threadId={threadId}
          state={state}
          configured={!setup}
        />
      )}
    </div>
  );
}
