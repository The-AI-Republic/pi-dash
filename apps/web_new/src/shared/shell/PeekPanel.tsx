// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared work-item peek panel (SHELL-016; written from parity/specs/shell.md
// §10; layouts from parity/specs/issues.md §14, ISS-166/168). The shell owns
// this panel shell — a side panel in the fullscreen portal with the title
// rendered as an editable field — and the issues epic reuses it for full
// peek with its own data and commit. Body content arrives as children, so
// this layer never reaches into feature data. Escape and outside-click both
// listen in capture so an open overlay still guards the peek while it
// dismisses itself; typing focus and open overlays always block the close.

import * as React from "react";
import { X } from "lucide-react";

import { IconButton } from "@pidash/kit";
import { FULL_SCREEN_PORTAL_ID, OverlayPortal } from "./document.js";

/** Panel presentations (ISS-166): docked side (default), centered modal, full-screen. */
export type PeekLayout = "side" | "modal" | "full";

/** Overlay roles whose presence blocks a peek close (ISS-168). */
const OPEN_OVERLAY = '[role="dialog"], [role="menu"], [role="listbox"]';

/** Surfaces an outside press ignores besides the panel (ISS-168). */
const OUTSIDE_IGNORED = `${OPEN_OVERLAY}, [data-testid="toast-stack"]`;

/** Focus inside an editable control blocks the Escape close (ISS-168). */
function isTypingTarget(node: Element | null): boolean {
  if (!(node instanceof HTMLElement)) return false;
  if (node.isContentEditable) return true;
  const tag = node.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

/** Tab order candidates inside a trapping panel. */
const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Tabbable descendants, in document order, minus hidden subtrees. */
function focusableWithin(root: HTMLElement | null): HTMLElement[] {
  if (root === null) return [];
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (node) => node.closest("[hidden], [aria-hidden='true']") === null
  );
}

/**
 * Whether an overlay other than `own` itself is open. Only the caller's
 * own panel is discounted — a picker or nested peek inside it still
 * guards the close.
 */
function otherOverlayOpen(own: Element | null | undefined): boolean {
  return Array.from(document.querySelectorAll(OPEN_OVERLAY)).some((node) => node !== own);
}

/**
 * Whether an Escape close must stand down: focus sits in an editable
 * control, or a dialog, menu, or picker is open besides this panel.
 */
export function peekEscapeBlocked(own?: Element | null | undefined): boolean {
  if (isTypingTarget(document.activeElement)) return true;
  return otherOverlayOpen(own);
}

/**
 * Whether an outside press must stand down: an overlay is open besides
 * this panel, even when the press itself lands outside it.
 */
export function peekOutsideBlocked(target: EventTarget | null, own?: Element | null | undefined): boolean {
  if (target instanceof Element && target.closest(OUTSIDE_IGNORED) !== null) return true;
  return otherOverlayOpen(own);
}

export interface PeekPanelProps {
  /** Accessible name for the panel; work-item panels keep the default. */
  label?: string | undefined;
  /** Test handle for the panel; work-item panels keep the default. */
  testId?: string | undefined;
  /** Presentation; side-peek unless the caller switches (ISS-166). */
  layout?: PeekLayout | undefined;
  /**
   * Accessibility role for the panel. Work-item peeks on workspace
   * surfaces keep the plain complementary region; a surface that must be
   * a modal dialog (the public issue overlay) passes `"dialog"` with a
   * name, and its own guards then ignore the panel itself.
   */
  role?: "dialog" | undefined;
  /** A nested panel paints above its parent. */
  stacked?: boolean | undefined;
  /** Context line above the title (e.g. the work-item key). */
  eyebrow?: React.ReactNode;
  /** Current title; the field commits through onCommitTitle. */
  title: string;
  /** Accessible name for the title field. */
  titleLabel: string;
  /** Render the title as a heading instead of an editable field. */
  titleAsHeading?: boolean | undefined;
  /** Skip the title block (loading and error states render their own body). */
  hideTitle?: boolean | undefined;
  onClose: () => void;
  /** Caller-known Escape suppression (embedded hosts, open dialogs). */
  suppressEscape?: boolean | undefined;
  /**
   * Trap keyboard focus in the panel and move it in on mount. Modal and
   * full presentations opt in; the docked side peek leaves the board
   * reachable, so it stays non-modal.
   */
  trapFocus?: boolean | undefined;
  /** Outside presses close through this; without it they are ignored. */
  onOutsideClick?: (() => void) | undefined;
  /** Commits a renamed title; without it edits stay local to the field. */
  onCommitTitle?: ((next: string) => void) | undefined;
  /** Controls beside the close button (e.g. copy link). */
  actions?: React.ReactNode;
  children?: React.ReactNode;
}

function panelClasses(layout: PeekLayout, stacked: boolean): string {
  if (layout === "full") {
    return `fixed inset-0 ${stacked ? "z-[60]" : "z-50"} flex flex-col gap-(--space-4) overflow-y-auto bg-(--surface) p-(--space-6) md:p-(--space-8)`;
  }
  if (layout === "modal") {
    return `fixed top-1/2 left-1/2 ${stacked ? "z-[60]" : "z-50"} flex max-h-[92vh] w-[min(48rem,94vw)] -translate-x-1/2 -translate-y-1/2 flex-col gap-(--space-4) overflow-y-auto rounded-(--radius-dialog) border border-(--border) bg-(--surface) p-(--space-6) shadow-dialog`;
  }
  return `fixed top-0 right-0 bottom-0 ${stacked ? "z-50" : "z-40"} flex w-[min(30rem,92vw)] flex-col gap-(--space-4) overflow-y-auto border-l border-(--border) bg-(--surface) p-(--space-6)`;
}

export function PeekPanel({
  label,
  testId,
  layout = "side",
  stacked = false,
  role,
  eyebrow,
  title,
  titleLabel,
  titleAsHeading = false,
  hideTitle = false,
  onClose,
  suppressEscape = false,
  trapFocus = false,
  onOutsideClick,
  onCommitTitle,
  actions,
  children,
}: PeekPanelProps): React.ReactNode {
  const [draft, setDraft] = React.useState(title);
  const panelRef = React.useRef<HTMLElement | null>(null);
  // An Escape revert resets the draft and blurs in the same tick; the
  // blur's commit would otherwise read the pre-revert draft and commit
  // the abandoned text, so the revert marks this and the commit stands
  // down once.
  const revertingRef = React.useRef(false);
  React.useEffect(() => {
    setDraft(title);
  }, [title]);
  React.useEffect(() => {
    // Capture runs before overlay key handling, so an open dialog still
    // guards the peek while Escape dismisses the dialog itself.
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || suppressEscape || peekEscapeBlocked(panelRef.current)) return;
      onClose();
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [onClose, suppressEscape]);
  React.useEffect(() => {
    if (onOutsideClick === undefined) return;
    const onPointerDown = (event: PointerEvent): void => {
      const target = event.target;
      if (
        target instanceof Node &&
        panelRef.current?.contains(target) !== true &&
        !peekOutsideBlocked(target, panelRef.current)
      ) {
        onOutsideClick();
      }
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [onOutsideClick]);
  React.useEffect(() => {
    if (!trapFocus) return;
    const panel = panelRef.current;
    if (panel === null) return;
    if (!panel.contains(document.activeElement)) panel.focus({ preventScroll: true });
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Tab") return;
      const focusables = focusableWithin(panel);
      if (focusables.length === 0) {
        event.preventDefault();
        panel.focus({ preventScroll: true });
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;
      if (event.shiftKey) {
        if (active === first || active === panel || !panel.contains(active)) {
          event.preventDefault();
          last?.focus();
        }
        return;
      }
      if (active === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [trapFocus]);

  const commit = (): void => {
    if (revertingRef.current) {
      revertingRef.current = false;
      return;
    }
    const next = draft.trim();
    if (next === "" || next === title) {
      setDraft(title);
      return;
    }
    onCommitTitle?.(next);
  };

  return (
    <OverlayPortal rootId={FULL_SCREEN_PORTAL_ID}>
      {layout === "modal" ? (
        <div
          data-testid="peek-backdrop"
          aria-hidden="true"
          className={`fixed inset-0 ${stacked ? "z-[55]" : "z-40"} bg-black/40`}
        />
      ) : null}
      <aside
        ref={panelRef}
        {...(role === undefined ? {} : { role })}
        {...(role === "dialog" && trapFocus ? { "aria-modal": "true" } : {})}
        {...(trapFocus ? { tabIndex: -1 } : {})}
        aria-label={label ?? "Work item preview"}
        data-testid={testId ?? "issue-preview"}
        data-peek-layout={layout}
        className={panelClasses(layout, stacked)}
      >
        <div className="flex items-start justify-between gap-(--space-4)">
          <p className="text-caption min-w-0 flex-1 truncate pt-(--space-1) text-(--text-muted)">{eyebrow}</p>
          <div className="flex shrink-0 items-center gap-(--space-1)">
            {actions}
            <IconButton label="Close preview" size="small" onClick={onClose}>
              <X size={16} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
          </div>
        </div>
        {hideTitle ? null : titleAsHeading ? (
          <h4 className="text-h3 w-full px-(--space-2) py-(--space-1) text-(--text)">{title}</h4>
        ) : (
          <input
            aria-label={titleLabel}
            value={draft}
            onChange={(event) => {
              setDraft(event.target.value);
            }}
            onBlur={commit}
            onKeyDown={(event) => {
              if (event.key === "Enter") commit();
              // Escape reverts the edit instead of closing: the typing
              // guard above already stood the close down, and the blur
              // hands the next Escape back to the panel.
              if (event.key === "Escape") {
                setDraft(title);
                revertingRef.current = true;
                event.currentTarget.blur();
              }
            }}
            className="text-h3 w-full rounded-(--radius-control) border border-transparent bg-transparent px-(--space-2) py-(--space-1) text-(--text) hover:border-(--border) focus:border-(--accent) focus:outline-none"
          />
        )}
        {children}
      </aside>
    </OverlayPortal>
  );
}
