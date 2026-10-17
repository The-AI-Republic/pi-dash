// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";
import { rowDomId } from "./fresh.js";
import { HighlightProvider, useHighlight, type HighlightOptions } from "./highlight.js";

function renderProbe(): {
  text: () => string;
  highlight: (id: string, options?: HighlightOptions) => void;
  clear: () => void;
} {
  let highlight!: (id: string, options?: HighlightOptions) => void;
  let clear!: () => void;
  function Probe(): React.ReactElement {
    const handle = useHighlight();
    highlight = handle.highlight;
    clear = handle.clearHighlight;
    return (
      <>
        <p>{`highlighted:${handle.highlightedId ?? "none"}`}</p>
        <article id={rowDomId("row-1")}>Row one</article>
      </>
    );
  }
  render(
    <HighlightProvider>
      <Probe />
    </HighlightProvider>
  );
  return {
    text: () => screen.getByText(/^highlighted:/).textContent ?? "",
    highlight: (id, options) => {
      act(() => {
        highlight(id, options);
      });
    },
    clear: () => {
      act(() => {
        clear();
      });
    },
  };
}

describe("HighlightProvider", () => {
  it("starts with no highlight and renders its children", () => {
    const h = renderProbe();
    expect(h.text()).toBe("highlighted:none");
    expect(screen.getByText("Row one")).toBeInTheDocument();
  });

  it("highlights and clears on demand", () => {
    const h = renderProbe();
    h.highlight("row-1");
    expect(h.text()).toBe("highlighted:row-1");
    h.clear();
    expect(h.text()).toBe("highlighted:none");
  });

  it("scrolls the row into view when asked", () => {
    const h = renderProbe();
    const row = document.getElementById(rowDomId("row-1"))!;
    const seen: unknown[] = [];
    row.scrollIntoView = ((options: unknown) => {
      seen.push(options);
    }) as Element["scrollIntoView"];
    h.highlight("row-1", { scroll: true });
    expect(h.text()).toBe("highlighted:row-1");
    expect(seen).toEqual([{ block: "nearest" }]);
  });

  it("stays silent when the row or the scroll primitive is missing", () => {
    const h = renderProbe();
    h.highlight("row-gone", { scroll: true });
    expect(h.text()).toBe("highlighted:row-gone");
    const row = document.getElementById(rowDomId("row-1"))!;
    expect(row.scrollIntoView).toBeUndefined();
    h.highlight("row-1", { scroll: true });
    expect(h.text()).toBe("highlighted:row-1");
  });

  it("clears on a pointer press outside the row, keeps presses inside", () => {
    const h = renderProbe();
    h.highlight("row-1");
    fireEvent.pointerDown(screen.getByText("Row one"));
    expect(h.text()).toBe("highlighted:row-1");
    fireEvent.pointerDown(document.body);
    expect(h.text()).toBe("highlighted:none");
  });

  it("clears on any pointer press when the row is gone", () => {
    const h = renderProbe();
    h.highlight("row-gone");
    fireEvent.pointerDown(document.body);
    expect(h.text()).toBe("highlighted:none");
  });

  it("throws outside the provider", () => {
    function Probe(): React.ReactElement {
      useHighlight();
      return <p>must not render</p>;
    }
    expect(() => render(<Probe />)).toThrow("useHighlight must be used under HighlightProvider.");
  });
});
