/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
import { Wrench } from "lucide-react";
import { Link } from "react-router";
import { MarkdownRenderer } from "@/components/ui/markdown-to-component";

export interface ChatMessageLink {
  url_path: string;
  label?: string;
}

/**
 * Tool-activity row shared by chat surfaces. Callers pass domain-specific
 * links (e.g. the assistant's payload links); runner chats can omit them.
 */
export function ChatToolActivity({ content, links = [] }: { content: string; links?: ChatMessageLink[] }) {
  return (
    <div className="flex items-start gap-2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-12 text-secondary">
      <Wrench className="mt-0.5 size-3.5 shrink-0" />
      <div>
        <span>{content}</span>
        {links.map((l) => (
          <Link key={l.url_path} to={l.url_path} className="ml-2 text-accent-primary hover:underline">
            {l.label ?? "View"}
          </Link>
        ))}
      </div>
    </div>
  );
}

export interface ChatMessageProps {
  role: string;
  content: string;
  status?: string;
  /** Domain-specific rendering for tool_call / tool_result rows. */
  toolActivity?: ReactNode;
}

/**
 * Shared chat bubble used by both the assistant and the runner agent chats so
 * presentation (markdown, streaming placeholder, error/tool styling) is
 * improved in one place. Domain components map their message type onto these
 * props and supply any tool-activity rendering.
 */
export function ChatMessage({ role, content, status, toolActivity }: ChatMessageProps) {
  switch (role) {
    case "user":
      return (
        <div className="flex justify-end">
          <div className="max-w-[72%] rounded-md bg-accent-primary px-3 py-2 text-13 whitespace-pre-wrap text-on-color">
            {content}
          </div>
        </div>
      );
    case "assistant":
      return (
        <div className="flex justify-start">
          <div className="max-w-[80%] rounded-md bg-layer-1 px-3 py-2 text-13 text-primary">
            {content ? (
              <MarkdownRenderer markdown={content} />
            ) : (
              <span className="text-secondary">{status === "streaming" ? "…" : ""}</span>
            )}
          </div>
        </div>
      );
    case "tool_call":
    case "tool_result":
      return toolActivity ?? null;
    case "error":
      return (
        <div className="border-danger/40 bg-danger/5 text-danger rounded-md border px-3 py-2 text-12">
          {content || "The assistant hit an error."}
        </div>
      );
    default:
      // Unknown roles (e.g. runner system/status rows) render as a plain bubble.
      return (
        <div className="flex justify-start">
          <div className="max-w-[72%] rounded-md bg-layer-1 px-3 py-2 text-13 text-primary">
            <div className="whitespace-pre-wrap">{content || status}</div>
          </div>
        </div>
      );
  }
}
