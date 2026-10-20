// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-text work-item description (parity/specs/issues.md §13, ISS-147).
// Reads render through the shared sanitized view; the first click loads
// the Tiptap chunk and swaps in the editor. Edits autosave through the
// shared pipeline: 1.5s after typing stops, on blur, and on unmount.
// Emissions identical to the last saved HTML never send. A restore
// (from the version history) closes the editor and shows the version
// at once; the server confirms it on re-read.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

import { isStoredHtmlEmpty, RichTextEditor, RichTextView, storedTextContent } from "../../../shared/editor/index.js";
import type { SavePipeline } from "./savePipeline.js";

/** Idle delay before a description edit autosaves. */
export const DESCRIPTION_AUTOSAVE_MS = 1500;

export const DESCRIPTION_LABEL = "Description";
export const DESCRIPTION_EDIT_LABEL = "Edit description";
export const DESCRIPTION_ADD_LABEL = "Add a description";
export const DESCRIPTION_PLACEHOLDER = "Write a description…";

/** A version restore the field is showing ahead of the server. */
export interface RestoredDescription {
  html: string;
  /** Bumps on every restore so repeats re-apply. */
  token: number;
}

export interface IssueDescriptionFieldProps {
  /** Server-side description HTML. */
  value: string;
  /** True for archived, synced, or guest views: read-only. */
  disabled: boolean;
  /** Shared title-plus-description save pipeline. */
  pipeline: SavePipeline;
  /** Persist HTML; settles when the write settles. */
  onSave: (html: string) => Promise<unknown>;
  /** Restore in flight, shown until the server confirms it. */
  restored?: RestoredDescription | null | undefined;
  /** Resolve a stored image asset id to its served source (ISS-127). */
  resolveAsset?: ((src: string) => string) | undefined;
}

export function IssueDescriptionField({
  value,
  disabled,
  pipeline,
  onSave,
  restored = null,
  resolveAsset,
}: IssueDescriptionFieldProps): React.ReactElement | null {
  const [editing, setEditing] = React.useState(false);
  const shown = restored?.html ?? value;
  const savedRef = React.useRef(value);
  const latestRef = React.useRef(value);
  const dirtyRef = React.useRef(false);
  // The editor echoes its normalized content on mount. The first
  // emission per mount is adopted as the baseline when it carries the
  // same text, so opening the editor never schedules a save.
  const echoRef = React.useRef(true);
  const openHtmlRef = React.useRef(value);
  const saveRef = React.useRef(onSave);
  saveRef.current = onSave;
  const pipelineRef = React.useRef(pipeline);
  pipelineRef.current = pipeline;
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearTimer = (): void => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  };

  const flush = React.useCallback((): void => {
    clearTimer();
    if (!dirtyRef.current) return;
    // Dirty implies latest differs from saved: emissions identical to
    // the saved HTML return early in onChange, and every savedRef
    // assignment clears the flag.
    const html = latestRef.current;
    pipelineRef.current.markSaving();
    void saveRef.current(html).then(
      () => {
        savedRef.current = html;
        dirtyRef.current = false;
        pipelineRef.current.markSaved();
      },
      () => {
        // Stays dirty: the next edit, blur, or unmount retries.
        pipelineRef.current.markSaved();
      }
    );
  }, []);

  const flushRef = React.useRef(flush);
  flushRef.current = flush;

  React.useEffect(() => {
    if (!editing) return;
    echoRef.current = true;
    openHtmlRef.current = shown;
  }, [editing]);

  // Losing edit rights mid-edit (archive, role change) drops the pending
  // buffer: the write would only come back refused.
  React.useEffect(() => {
    if (!disabled) return;
    dirtyRef.current = false;
    clearTimer();
    setEditing(false);
  }, [disabled]);

  // Server movements converge the field: when the server holds our
  // latest HTML we are clean, otherwise a save is still owed and one
  // is scheduled when no debounce is already pending.
  React.useEffect(() => {
    if (latestRef.current === value) {
      savedRef.current = value;
      dirtyRef.current = false;
      clearTimer();
      return;
    }
    if (timerRef.current === null) {
      timerRef.current = setTimeout(() => flushRef.current(), DESCRIPTION_AUTOSAVE_MS);
    }
  }, [value]);

  // A restore replaces the buffer wholesale and closes the editor; the
  // view shows the version at once, ahead of the server round-trip.
  // Keyed on the token alone: the render's own object carries the HTML.
  const restoredToken = restored?.token;
  React.useEffect(() => {
    if (restored === null || restored === undefined) return;
    const html = restored.html;
    savedRef.current = html;
    latestRef.current = html;
    dirtyRef.current = false;
    clearTimer();
    setEditing(false);
  }, [restoredToken]);

  React.useEffect(
    () => () => {
      flushRef.current();
    },
    []
  );

  if (disabled && isStoredHtmlEmpty(shown)) return null;

  if (disabled || !editing) {
    if (!disabled && isStoredHtmlEmpty(shown)) {
      return (
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="text-body cursor-text rounded-(--radius-control) px-(--space-2) py-(--space-4) text-left text-(--text-muted) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:outline-none"
        >
          {DESCRIPTION_ADD_LABEL}
        </button>
      );
    }
    const interactive = !disabled;
    return (
      <div
        role={interactive ? "button" : undefined}
        tabIndex={interactive ? 0 : undefined}
        aria-label={interactive ? DESCRIPTION_EDIT_LABEL : undefined}
        className={
          interactive
            ? "cursor-text rounded-(--radius-control) px-(--space-2) py-(--space-4) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:outline-none"
            : "px-(--space-2) py-(--space-4)"
        }
        onClick={
          interactive
            ? (event: React.MouseEvent<HTMLDivElement>) => {
                // Stored links keep working: only plain-surface clicks edit.
                if ((event.target as HTMLElement).closest("a") !== null) return;
                setEditing(true);
              }
            : undefined
        }
        onKeyDown={
          interactive
            ? (event: React.KeyboardEvent<HTMLDivElement>) => {
                if (event.key !== "Enter" && event.key !== " ") return;
                event.preventDefault();
                setEditing(true);
              }
            : undefined
        }
      >
        <RichTextView html={shown} label={DESCRIPTION_LABEL} resolveAsset={resolveAsset} />
      </div>
    );
  }

  return (
    <div onBlur={() => flushRef.current()} className="px-(--space-2) py-(--space-4)">
      <React.Suspense
        fallback={
          <div role="status" aria-label="Loading editor" className="flex flex-col gap-(--space-4)">
            <Skeleton height="14px" width="90%" />
            <Skeleton height="14px" width="70%" />
            <Skeleton height="14px" width="80%" />
          </div>
        }
      >
        <RichTextEditor
          initialHtml={shown}
          label={DESCRIPTION_LABEL}
          placeholder={DESCRIPTION_PLACEHOLDER}
          autoFocus
          {...(resolveAsset === undefined ? {} : { resolveAsset })}
          onChange={(html: string) => {
            if (echoRef.current) {
              echoRef.current = false;
              if (storedTextContent(html) === storedTextContent(openHtmlRef.current)) {
                savedRef.current = html;
                latestRef.current = html;
                return;
              }
            }
            // Back at the saved HTML: nothing is owed. Settle the
            // pipeline and drop the pending debounce, or it would send
            // the stale buffer captured before this emission.
            if (html === savedRef.current) {
              latestRef.current = html;
              dirtyRef.current = false;
              clearTimer();
              pipeline.markSaved();
              return;
            }
            latestRef.current = html;
            dirtyRef.current = true;
            pipeline.markSaving();
            clearTimer();
            timerRef.current = setTimeout(() => flushRef.current(), DESCRIPTION_AUTOSAVE_MS);
          }}
        />
      </React.Suspense>
    </div>
  );
}
