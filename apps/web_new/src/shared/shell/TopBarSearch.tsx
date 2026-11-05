// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Top-bar search box (shell spec, SHELL-081). An always-visible field that
// opens an inline (non-dialog) results panel on focus: the same commands
// plus grouped server hits as the modal palette. Escape clears the term and
// closes in one press; an outside click closes; closing always resets the
// term. Opening the modal palette resets this panel with it.
// DOM contract for the area-gate driver (NEWFRONT-295): a textbox with
// placeholder TOPBAR_SEARCH_PLACEHOLDER; the open panel is role=listbox
// named "Top bar search results" (never a dialog); rows are role=option
// with a single aria-selected=true; the results heading pulses mid-flight.
//
// Size shape (NEWFRONT-523, size gate): only the field, the listbox shell
// and the keyboard flow stay eager. The rows — command filtering, the
// server search query and the option buttons — load lazily with
// TopBarSearchPanel on first open and report their row count and
// activation back through the panel API, so arrows and Enter behave
// exactly as if the panel had always been there.

import * as React from "react";

import { useCommandPalette } from "../commands/index.js";
import { usePaletteKeys } from "./paletteKeys.js";
import { useTopSearchFocus } from "./topSearchFocus.js";
import type { PaletteSearchScope, SearchResultHit } from "../commands/index.js";
import type { TopBarSearchPanelApi } from "./TopBarSearchPanel.js";

// Deferred below the initial shell chain (Quality gates budget): the rows
// mount on first panel open; the listbox shell below renders at once so
// the open state is never late.
const LazySearchPanel = React.lazy(() =>
  import("./TopBarSearchPanel.js").then((module) => ({ default: module.TopBarSearchPanel }))
);

/** The always-visible top-bar field prompt (SHELL-081). */
export const TOPBAR_SEARCH_PLACEHOLDER = "Search commands...";

// Box widths (NEWPIDASH-7): narrow at rest, wide while the results panel
// is open, matching the old app's sizes. Kept in style props (not width
// utilities) so the open width can cap itself to the viewport.
const SEARCH_BOX_NARROW_PX = 364;
const SEARCH_BOX_WIDE_PX = 554;

export interface TopBarSearchProps {
  /** Null renders commands only (shells without workspace context). */
  scope: PaletteSearchScope | null;
  /** Runs when a server hit activates; the panel is already closed. */
  onSelectSearchResult: (hit: SearchResultHit) => void;
}

export function TopBarSearch({ scope, onSelectSearchResult }: TopBarSearchProps): React.ReactElement {
  const [term, setTerm] = React.useState("");
  const [open, setOpen] = React.useState(false);
  const modalOpen = useCommandPalette((state) => state.open);
  const containerRef = React.useRef<HTMLDivElement>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const focusRequest = useTopSearchFocus((state) => state.request);
  const lastRequest = React.useRef(focusRequest);
  // The panel reports its rows here once its chunk lands; until then the
  // keyboard sees zero rows, so arrows and Enter safely do nothing.
  const panelApi = React.useRef<TopBarSearchPanelApi>({ count: 0, activate: () => undefined });
  const [panelCount, setPanelCount] = React.useState(0);

  const close = React.useCallback(() => {
    setOpen(false);
    setTerm("");
  }, []);

  const handlePanelApi = React.useCallback((api: TopBarSearchPanelApi) => {
    panelApi.current = api;
    setPanelCount(api.count);
  }, []);

  const handleActivate = React.useCallback((index: number) => {
    panelApi.current.activate(index);
  }, []);

  // The palette's focus entry bumps the counter; focusing opens the panel
  // through the input's own onFocus (SHELL-089). Compares against the
  // last seen counter (not a mount flag): StrictMode replays this effect
  // on mount, and a flag would focus the box on every page load.
  React.useEffect(() => {
    if (lastRequest.current === focusRequest) return;
    lastRequest.current = focusRequest;
    inputRef.current?.focus();
  }, [focusRequest]);

  React.useEffect(() => {
    if (modalOpen) close();
  }, [modalOpen, close]);

  React.useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      // Mounted while the panel is open, so the container always exists.
      if (!containerRef.current!.contains(event.target as Node)) close();
    };
    globalThis.document.addEventListener("pointerdown", onPointerDown);
    return () => {
      globalThis.document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open, close]);

  const { activeIndex, setActiveIndex, onKeyDown } = usePaletteKeys({
    count: panelCount,
    query: term,
    onActivate: handleActivate,
    // One press clears and closes together (SHELL-081/083).
    onClearQuery: close,
    onDismiss: close,
  });

  // Tabbing (or otherwise moving focus) out of the box closes the panel
  // and collapses it; focus landing on a row inside stays open. A null
  // target (an outside click the pointer handler already closed) closes.
  const handleBlur = (event: React.FocusEvent<HTMLDivElement>): void => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) close();
  };

  return (
    // The outer div reserves the narrow footprint in the top-bar flow but
    // yields below it when the bar is short of room (no shrink floor
    // anywhere in this chain), so narrow windows never scroll sideways.
    // The inner box overlays the footprint and grows symmetric about its
    // centre while open, so the items on either side never move. The panel
    // mounts inside the inner box with left/right pinned, so it always
    // matches the box width. The open width never exceeds the viewport
    // (minus the bar's own side padding) and never shrinks below the
    // footprint.
    <div
      ref={containerRef}
      onBlur={handleBlur}
      className="relative h-(--control-height) min-w-0"
      style={{ width: SEARCH_BOX_NARROW_PX, maxWidth: "100%" }}
    >
      <div
        data-testid="topbar-search-box"
        className="absolute top-0 left-1/2 z-10 h-full -translate-x-1/2 transition-[width] duration-(--motion-duration) ease-in-out motion-reduce:transition-none"
        style={{
          width: open ? `max(100%, min(${SEARCH_BOX_WIDE_PX}px, calc(100vw - var(--space-8))))` : "100%",
        }}
      >
        <input
          ref={inputRef}
          type="text"
          aria-label="Search commands"
          placeholder={TOPBAR_SEARCH_PLACEHOLDER}
          value={term}
          onChange={(event) => setTerm(event.target.value)}
          onFocus={() => setOpen(true)}
          // Re-clicking the focused box reopens: focus alone never refires
          // while the input already holds it (after Escape, say).
          onClick={() => setOpen(true)}
          onKeyDown={onKeyDown}
          className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
        />
        {open ? (
          <div
            role="listbox"
            aria-label="Top bar search results"
            className="shadow-dialog absolute top-full right-0 left-0 z-50 mt-(--space-2) max-h-80 overflow-auto rounded-(--radius-dialog) border border-(--border) bg-(--bg)"
          >
            <React.Suspense fallback={null}>
              <LazySearchPanel
                term={term}
                scope={scope}
                activeIndex={activeIndex}
                onActiveIndexChange={setActiveIndex}
                onTermChange={setTerm}
                onClose={close}
                onSelectSearchResult={onSelectSearchResult}
                onPanelApi={handlePanelApi}
              />
            </React.Suspense>
          </div>
        ) : null}
      </div>
    </div>
  );
}
