// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live page document (parity/specs/pages.md §6.5–§6.8, PAGE-046–049): the
// pane's view of the canvas — seeded from the stored body, re-read on every
// canvas change without a save, and able to bring an outline heading into
// view.

import { act, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { useLivePageDocument, type LivePageDocument } from "./pageDocumentLive.js";

const SCOPE = { workspaceSlug: "acme", projectId: null };

function Harness({ html, noCanvas = false }: { html: string; noCanvas?: boolean }): React.ReactElement {
  const canvasRef = React.useRef<HTMLDivElement | null>(null);
  const live = useLivePageDocument(canvasRef, html, SCOPE);
  return (
    <div>
      {noCanvas ? null : <div ref={canvasRef} data-testid="canvas" />}
      <output data-testid="state">{JSON.stringify(live)}</output>
      <button type="button" onClick={() => live.scrollToHeading({ level: 2, text: "Mid", levelIndex: 2 })}>
        scroll
      </button>
    </div>
  );
}

function state(): LivePageDocument {
  return JSON.parse(screen.getByTestId("state").textContent ?? "{}") as LivePageDocument;
}

function paint(markup: string): void {
  const canvas = screen.getByTestId("canvas");
  act(() => {
    canvas.innerHTML = markup;
  });
}

describe("useLivePageDocument", () => {
  it("seeds from the stored body until the canvas paints", () => {
    render(<Harness html="<h1>Stored</h1><p>one two</p>" />);
    expect(state().document.headings).toEqual([{ level: 1, text: "Stored", levelIndex: 1 }]);
    expect(state().document.stats.words).toBe(3);
  });

  it("re-reads the canvas when it changes, without a save", async () => {
    render(<Harness html="<p>before</p>" />);
    paint("<h1>Typed</h1><h2>Under it</h2><p>three words here</p><img src='https://x.test/a.png' alt='A'>");
    await waitFor(() =>
      expect(state().document).toMatchObject({
        headings: [
          { level: 1, text: "Typed", levelIndex: 1 },
          { level: 2, text: "Under it", levelIndex: 1 },
        ],
        stats: { words: 6, paragraphs: 1 },
      })
    );
    expect(state().document.assets).toHaveLength(1);
  });

  it("scopes asset rows to the page's workspace", async () => {
    render(<Harness html="" />);
    paint('<img src="asset-3">');
    await waitFor(() => expect(state().document.assets[0]?.viewHref).toBe("/api/assets/v2/workspaces/acme/asset-3/"));
  });

  it("scrolls the canvas to the heading the outline picked", () => {
    render(<Harness html="<h1>Top</h1><h2>Other</h2><h2>Mid</h2>" />);
    paint("<h1>Top</h1><h2>Other</h2><h2>Mid</h2>");
    const canvas = screen.getByTestId("canvas");
    const mid = canvas.querySelectorAll("h2")[1] as HTMLElement;
    const scroll = vi.fn();
    mid.scrollIntoView = scroll;
    act(() => {
      screen.getByRole("button", { name: "scroll" }).click();
    });
    expect(scroll).toHaveBeenCalledWith({ behavior: "smooth", block: "start" });
  });

  it("leaves the stored document in place where no observer exists", () => {
    const saved = globalThis.MutationObserver;
    // @ts-expect-error -- a runtime without MutationObserver.
    globalThis.MutationObserver = undefined;
    try {
      render(<Harness html="<h1>Stored</h1>" />);
      paint("<h1>Painted</h1>");
      expect(state().document.headings).toEqual([{ level: 1, text: "Stored", levelIndex: 1 }]);
    } finally {
      globalThis.MutationObserver = saved;
    }
  });

  it("drops a queued re-read when the canvas goes away", () => {
    const observers: Array<() => void> = [];
    class CapturedObserver {
      public constructor(private readonly callback: () => void) {
        observers.push(callback);
      }
      public observe(): void {
        return undefined;
      }
      public disconnect(): void {
        return undefined;
      }
    }
    const saved = globalThis.MutationObserver;
    const cancel = vi.spyOn(globalThis, "cancelAnimationFrame");
    // @ts-expect-error -- a stand-in observer that hands over its callback.
    globalThis.MutationObserver = CapturedObserver;
    try {
      const { unmount } = render(<Harness html="<h1>Stored</h1>" />);
      act(() => {
        // A second change inside the same frame joins the pending read.
        observers[0]?.();
        observers[0]?.();
      });
      unmount();
      expect(cancel).toHaveBeenCalled();
    } finally {
      globalThis.MutationObserver = saved;
      cancel.mockRestore();
    }
  });

  it("does nothing when the heading or the canvas is gone", () => {
    render(<Harness html="<h1>Top</h1>" />);
    act(() => {
      screen.getByRole("button", { name: "scroll" }).click();
    });
    expect(screen.getByTestId("canvas").querySelectorAll("h2")).toHaveLength(0);
  });

  it("does nothing before a canvas mounts", () => {
    render(<Harness html="<h1>Top</h1>" noCanvas />);
    expect(() => {
      act(() => {
        screen.getByRole("button", { name: "scroll" }).click();
      });
    }).not.toThrow();
  });
});
