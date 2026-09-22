/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IAssistantLink, IAssistantMessage, IAssistantSkippedServer } from "@pi-dash/types";
import { ChatMessage, ChatToolActivity } from "@/components/chat/message";
import { ToolServersSkippedNotice } from "@/components/assistant/tool-servers-skipped-notice";

export function AssistantMessage({ message }: { message: IAssistantMessage }) {
  // Client-synthesized notice (tool_servers_skipped) — render inline, not as a
  // chat bubble.
  if (message.role === "notice") {
    const servers = (message.payload?.servers ?? []) as IAssistantSkippedServer[];
    return <ToolServersSkippedNotice servers={servers} />;
  }
  const isTool = message.role === "tool_call" || message.role === "tool_result";
  const links = (message.payload?.links ?? []) as IAssistantLink[];
  return (
    <ChatMessage
      role={message.role}
      content={message.content}
      status={message.status}
      toolActivity={isTool ? <ChatToolActivity content={message.content} links={links} /> : undefined}
    />
  );
}
