/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Sparkles } from "lucide-react";
import { Link } from "react-router";
import { Button } from "@pi-dash/ui";

export function AssistantSetupCard() {
  return (
    <div className="mx-auto flex max-w-md flex-col items-center gap-3 rounded-lg border border-subtle bg-surface-1 px-6 py-8 text-center">
      <Sparkles className="size-6 text-accent-primary" />
      <div className="text-15 font-semibold text-primary">Set up your AI assistant</div>
      <p className="text-13 text-secondary">
        Bring your own LLM provider key to start chatting. The assistant can search, create, and update issues on your
        behalf — with exactly your permissions.
      </p>
      <Link to="/settings/profile/ai-assistant">
        <Button variant="primary" size="sm">
          Configure provider
        </Button>
      </Link>
    </div>
  );
}
