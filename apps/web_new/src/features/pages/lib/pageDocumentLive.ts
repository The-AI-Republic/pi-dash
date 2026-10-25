// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The live page document (parity/specs/pages.md §6.5–§6.8, PAGE-046–049).
// The navigation pane reads whatever is painted on the canvas, so headings,
// counts and assets follow typing without waiting for a save. A canvas
// without a live editing surface (a read-only page) still yields the stored
// body's document, and a body that appears later is picked up by watching
// the canvas for changes.

import * as React from "react";

import {
  parsePageDocument,
  type PageAssetScope,
  type PageDocument,
  type PageHeading,
  type PageHeadingLevel,
} from "./pagePane.js";

/** The document state plus the outline's scroll action. */
export interface LivePageDocument {
  document: PageDocument;
  scrollToHeading: (heading: PageHeading) => void;
}

/** Heading tags the canvas walk considers, in document order. */
const HEADING_SELECTOR = "h1, h2, h3";

/**
 * The region the pane describes: the body inside the canvas, never the
 * title, the formatting bar or the page's action mounts that share the
 * same container (PAGE-046–049, §6.5). The read-only body and the live
 * editor both paint it as `.editor-container`.
 */
export function pageBodyElement(canvas: HTMLElement): HTMLElement | null {
  return canvas.querySelector<HTMLElement>(".editor-container");
}

function headingElements(container: HTMLElement, level: PageHeadingLevel, levelIndex: number): HTMLElement | null {
  const matches = Array.from(container.querySelectorAll<HTMLElement>(HEADING_SELECTOR)).filter(
    (element) => element.tagName.toLowerCase() === `h${level}`
  );
  return matches[levelIndex - 1] ?? null;
}

/**
 * One canvas's live document. `storedHtml` seeds the state until the canvas
 * paints, and every mutation of the canvas (typing, paste, an image being
 * removed) re-reads it, coalesced to at most once per animation frame.
 */
export function useLivePageDocument(
  canvasRef: React.RefObject<HTMLElement | null>,
  storedHtml: string,
  scope: PageAssetScope
): LivePageDocument {
  const { workspaceSlug, projectId } = scope;
  const [document, setDocument] = React.useState<PageDocument>(() =>
    parsePageDocument(storedHtml, { workspaceSlug, projectId })
  );

  // Reading the canvas rather than the stored body is what makes the pane
  // live. The read is scoped to the body: the title shares the canvas and
  // is not part of the document the outline, preview and counts describe.
  // A canvas whose body has not painted yet (cold load, no editor) leaves
  // the stored document in place instead of reporting empty.
  const read = React.useCallback((): void => {
    const canvas = canvasRef.current;
    if (canvas === null) {
      setDocument(parsePageDocument(storedHtml, { workspaceSlug, projectId }));
      return;
    }
    const body = pageBodyElement(canvas) ?? canvas;
    const painted = body.innerHTML.trim() === "" ? storedHtml : body.innerHTML;
    setDocument(parsePageDocument(painted, { workspaceSlug, projectId }));
  }, [canvasRef, storedHtml, workspaceSlug, projectId]);

  React.useEffect(() => {
    read();
    const canvas = canvasRef.current;
    if (canvas === null || typeof MutationObserver === "undefined") return;
    let frame: number | null = null;
    const observer = new MutationObserver(() => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        read();
      });
    });
    observer.observe(canvas, { childList: true, subtree: true, characterData: true });
    return () => {
      observer.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, [canvasRef, read]);

  const scrollToHeading = React.useCallback(
    (heading: PageHeading): void => {
      const canvas = canvasRef.current;
      if (canvas === null) return;
      const body = pageBodyElement(canvas) ?? canvas;
      const element = headingElements(body, heading.level, heading.levelIndex);
      if (element !== null && typeof element.scrollIntoView === "function") {
        element.scrollIntoView({ behavior: "smooth", block: "start" });
      }
    },
    [canvasRef]
  );

  return { document, scrollToHeading };
}
