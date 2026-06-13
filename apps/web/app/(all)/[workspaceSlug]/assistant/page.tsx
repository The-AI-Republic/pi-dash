/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Sparkles } from "lucide-react";
import { useParams } from "react-router";
import { AssistantSetupCard } from "@/components/assistant/setup-card";
import { useLLMConfig } from "@/components/assistant/use-llm-config";
import { useStartAssistantChat } from "@/components/assistant/use-start-chat";
import { ChatComposer } from "@/components/chat/composer";

const AssistantIndexPage = observer(function AssistantIndexPage() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const [draft, setDraft] = useState("");
  const { needsSetup } = useLLMConfig();
  const { start, starting } = useStartAssistantChat(workspaceSlug);

  return (
    <div className="flex h-full w-full overflow-auto px-4">
      {/* m-auto (not items/justify-center) so content taller than the panel stays scrollable */}
      <div className="m-auto w-full max-w-2xl py-8">
        {needsSetup ? (
          <AssistantSetupCard />
        ) : (
          <>
            <div className="mb-6 flex flex-col items-center gap-2 text-center">
              <Sparkles className="size-8 text-accent-primary" />
              <h1 className="text-20 font-semibold text-primary">How can I help you today?</h1>
              <p className="text-13 text-secondary">Ask about your issues, create work, or start a coding run.</p>
            </div>
            <ChatComposer
              draft={draft}
              onDraftChange={setDraft}
              onSend={() => void start(draft)}
              sending={starting}
              bordered={false}
            />
          </>
        )}
      </div>
    </div>
  );
});

export default AssistantIndexPage;
