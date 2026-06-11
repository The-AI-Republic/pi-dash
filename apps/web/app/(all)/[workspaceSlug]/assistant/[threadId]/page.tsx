/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "react-router";
import { AssistantChatRoot } from "@/components/assistant/chat-root";

const AssistantThreadPage = observer(function AssistantThreadPage() {
  const { workspaceSlug, threadId } = useParams<{ workspaceSlug: string; threadId: string }>();
  if (!workspaceSlug || !threadId) return null;
  return (
    <div className="mx-auto h-full w-full max-w-3xl px-4 py-6">
      <AssistantChatRoot slug={workspaceSlug} threadId={threadId} />
    </div>
  );
});

export default AssistantThreadPage;
