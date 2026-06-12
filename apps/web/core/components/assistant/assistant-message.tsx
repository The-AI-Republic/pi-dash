/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Wrench } from "lucide-react";
import { Link } from "react-router";
import type { IAssistantLink, IAssistantMessage } from "@pi-dash/types";
import { MarkdownRenderer } from "@/components/ui/markdown-to-component";

function ToolActivity({ message }: { message: IAssistantMessage }) {
  const links = (message.payload?.links ?? []) as IAssistantLink[];
  return (
    <div className="flex items-start gap-2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-12 text-secondary">
      <Wrench className="mt-0.5 size-3.5 shrink-0" />
      <div>
        <span>{message.content}</span>
        {links.map((l) => (
          <Link key={l.url_path} to={l.url_path} className="ml-2 text-accent-primary hover:underline">
            View
          </Link>
        ))}
      </div>
    </div>
  );
}

export function AssistantMessage({ message }: { message: IAssistantMessage }) {
  switch (message.role) {
    case "user":
      return (
        <div className="flex justify-end">
          <div className="max-w-[72%] rounded-md bg-accent-primary px-3 py-2 text-13 whitespace-pre-wrap text-on-color">
            {message.content}
          </div>
        </div>
      );
    case "assistant":
      return (
        <div className="flex justify-start">
          <div className="max-w-[80%] rounded-md bg-layer-1 px-3 py-2 text-13 text-primary">
            {message.content ? (
              <MarkdownRenderer markdown={message.content} />
            ) : (
              <span className="text-secondary">{message.status === "streaming" ? "…" : ""}</span>
            )}
          </div>
        </div>
      );
    case "tool_call":
    case "tool_result":
      return <ToolActivity message={message} />;
    case "error":
      return (
        <div className="border-danger/40 bg-danger/5 text-danger rounded-md border px-3 py-2 text-12">
          {message.content || "The assistant hit an error."}
        </div>
      );
    default:
      return null;
  }
}
