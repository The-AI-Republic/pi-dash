// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { Markdown, MarkdownSpans } from "./Markdown";

describe("Markdown", () => {
  it("renders formatted prose as elements, never raw HTML", () => {
    const { container } = render(<Markdown source={"Hello **world**\n\n<i>raw</i>"} />);
    expect(container.querySelector("strong")).toHaveTextContent("world");
    expect(container.querySelector("i")).toBeNull();
    expect(container).toHaveTextContent("<i>raw</i>");
  });

  it("renders headings, code, quotes, lists and rules", () => {
    const { container } = render(<Markdown source={"# Title\n\n```ts\ncode\n```\n\n> quote\n\n- one\n- two\n\n***"} />);
    expect(container.querySelector("h1")).toHaveTextContent("Title");
    expect(container.querySelector("pre code")).toHaveTextContent("code");
    expect(container.querySelector("blockquote")).toHaveTextContent("quote");
    expect(container.querySelectorAll("li")).toHaveLength(2);
    expect(container.querySelector("hr")).not.toBeNull();
  });

  it("uses the middle type step for a second-level heading", () => {
    const { container } = render(<Markdown source="## Two" />);
    expect(container.querySelector("h2")).toHaveClass("text-[1.12em]");
  });

  it("keeps later heading levels inside the type scale", () => {
    const { container } = render(<Markdown source={"#### Four\n\n##### Five\n\n###### Six"} />);
    for (const level of [4, 5, 6]) {
      expect(container.querySelector(`h${level}`)).toHaveTextContent(
        level === 4 ? "Four" : level === 5 ? "Five" : "Six"
      );
    }
  });

  it("marks list numbering and links safely", () => {
    const { container } = render(<Markdown source={"2. two\n3. [three](https://x.test)"} />);
    expect(container.querySelector("ol")).toHaveAttribute("start", "2");
    const link = screen.getByRole("link", { name: "three" });
    expect(link).toHaveAttribute("href", "https://x.test");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    expect(link).toHaveAttribute("target", "_blank");
  });

  it("renders each list entry's paragraph without outer margins", () => {
    const { container } = render(<Markdown source={"- a\n- b"} />);
    const paragraphs = container.querySelectorAll("li p");
    expect(paragraphs).toHaveLength(2);
    expect(paragraphs[0]).toHaveClass("m-0");
    expect(paragraphs[1]).toHaveClass("m-0");
  });

  it("renders nothing for an empty source", () => {
    const { container } = render(<Markdown source={"  \n\n"} className="extra" />);
    expect(container.firstElementChild).toBeNull();
  });

  it("carries the caller's class", () => {
    render(<Markdown source="text" className="prose-test" />);
    expect(screen.getByTestId("markdown")).toHaveClass("prose-test");
  });
});

describe("MarkdownSpans", () => {
  it("renders every span kind", () => {
    const { container } = render(
      <MarkdownSpans
        spans={[
          { type: "text", text: "a" },
          { type: "code", text: "b" },
          { type: "strong", spans: [{ type: "text", text: "c" }] },
          { type: "emphasis", spans: [{ type: "text", text: "d" }] },
          { type: "strike", spans: [{ type: "text", text: "e" }] },
          { type: "link", href: "/x", spans: [{ type: "text", text: "f" }] },
          { type: "break" },
        ]}
      />
    );
    expect(container).toHaveTextContent("abcdef");
    expect(container.querySelector("code")).toHaveTextContent("b");
    expect(container.querySelector("strong")).toHaveTextContent("c");
    expect(container.querySelector("em")).toHaveTextContent("d");
    expect(container.querySelector("s")).toHaveTextContent("e");
    expect(container.querySelector("a")).toHaveAttribute("href", "/x");
    expect(container.querySelector("br")).not.toBeNull();
  });
});
