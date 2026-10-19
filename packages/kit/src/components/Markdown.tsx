// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Formatted text from markdown (UI kit and editor page). Chat replies and
// other agent-written prose arrive as markdown; this renders the parsed
// blocks as React elements through the type tokens, so nothing written by
// a model is ever injected as HTML. Links keep browsing schemes only.

import * as React from "react";

import { cn } from "../lib/cn";
import { parseMarkdown, type MarkdownBlock, type MarkdownListItem, type MarkdownSpan } from "../lib/markdown";

export interface MarkdownProps {
  /** Markdown source; an empty string renders nothing. */
  source: string;
  className?: string;
}

/** Render one markdown span, including its nested spans. */
export function MarkdownSpans({ spans }: { spans: readonly MarkdownSpan[] }): React.ReactElement {
  return (
    <>
      {spans.map((span, index) => {
        switch (span.type) {
          case "text":
            return <React.Fragment key={index}>{span.text}</React.Fragment>;
          case "code":
            return (
              <code
                key={index}
                className="font-mono rounded-(--radius-control) bg-(--subtle) px-(--space-1) py-px text-[0.94em] text-(--text)"
              >
                {span.text}
              </code>
            );
          case "strong":
            return (
              <strong key={index} className="font-semibold">
                <MarkdownSpans spans={span.spans} />
              </strong>
            );
          case "emphasis":
            return (
              <em key={index}>
                <MarkdownSpans spans={span.spans} />
              </em>
            );
          case "strike":
            return (
              <s key={index}>
                <MarkdownSpans spans={span.spans} />
              </s>
            );
          case "link":
            return (
              <a
                key={index}
                href={span.href}
                target="_blank"
                rel="noopener noreferrer"
                className="text-(--accent) underline underline-offset-2 hover:text-(--accent-hover)"
              >
                <MarkdownSpans spans={span.spans} />
              </a>
            );
          case "break":
            return <br key={index} />;
        }
      })}
    </>
  );
}

function ListItem({ item }: { item: MarkdownListItem }): React.ReactElement {
  return (
    <li className="pl-(--space-1)">
      {item.blocks.map((block, index) => (
        <MarkdownBlockView key={index} block={block} compact={block.type === "paragraph"} />
      ))}
    </li>
  );
}

function MarkdownBlockView({
  block,
  compact = false,
}: {
  block: MarkdownBlock;
  compact?: boolean;
}): React.ReactElement {
  switch (block.type) {
    case "paragraph":
      return (
        <p className={compact ? "m-0" : "my-(--space-2) first:mt-0 last:mb-0"}>
          <MarkdownSpans spans={block.spans} />
        </p>
      );
    case "heading": {
      const size =
        block.level === 1
          ? "text-[1.24em] font-semibold"
          : block.level === 2
            ? "text-[1.12em] font-semibold"
            : "text-[1.04em] font-semibold";
      const Heading = `h${block.level}` as keyof React.JSX.IntrinsicElements;
      return (
        <Heading className={cn("my-(--space-3) first:mt-0 last:mb-0", size)}>
          <MarkdownSpans spans={block.spans} />
        </Heading>
      );
    }
    case "code":
      return (
        <pre className="font-mono text-meta my-(--space-2) overflow-x-auto rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-3)">
          <code>{block.text}</code>
        </pre>
      );
    case "quote":
      return (
        <blockquote className="my-(--space-2) border-l-2 border-(--border) pl-(--space-3) text-(--text-muted)">
          {block.blocks.map((child, index) => (
            <MarkdownBlockView key={index} block={child} />
          ))}
        </blockquote>
      );
    case "list": {
      const Tag = block.ordered ? "ol" : "ul";
      return (
        <Tag
          className={cn(
            "my-(--space-2) flex flex-col gap-(--space-1) pl-(--space-6) first:mt-0 last:mb-0",
            block.ordered ? "list-decimal" : "list-disc"
          )}
          {...(block.ordered ? { start: block.start } : {})}
        >
          {block.items.map((item, index) => (
            <ListItem key={index} item={item} />
          ))}
        </Tag>
      );
    }
    case "rule":
      return <hr className="my-(--space-4) border-(--border)" />;
  }
}

/** Markdown as formatted React content. */
export function Markdown({ source, className }: MarkdownProps): React.ReactElement | null {
  const blocks = React.useMemo(() => parseMarkdown(source), [source]);
  if (blocks.length === 0) return null;
  return (
    <div data-testid="markdown" className={cn("min-w-0 break-words", className)}>
      {blocks.map((block, index) => (
        <MarkdownBlockView key={index} block={block} />
      ))}
    </div>
  );
}
