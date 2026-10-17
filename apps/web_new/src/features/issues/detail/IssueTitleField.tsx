// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline work-item title (parity/specs/issues.md §13, ISS-145). Edits
// autosave through the shared pipeline: 1.5s after typing stops, on
// blur, and on unmount. Empty input reverts and is never sent; the
// 255 cap carries a live counter. Read-only callers (archived,
// synced, guests) render the title as a heading instead. Peek reuses
// this field around its own pipeline.

import * as React from "react";

import type { SavePipeline } from "./savePipeline.js";

/** Idle delay before a title edit autosaves. */
export const TITLE_AUTOSAVE_MS = 1500;
/** Longest title the server accepts. */
export const TITLE_MAX_LENGTH = 255;

export const TITLE_LABEL = "Title";
export const TITLE_PLACEHOLDER = "Add a title";
export const TITLE_REQUIRED_MESSAGE = "The title can't be empty.";

export interface IssueTitleFieldProps {
  /** Server-side name the field edits. */
  value: string;
  /** True for archived, synced, or guest views: heading, no input. */
  disabled: boolean;
  /** Shared title-plus-description save pipeline. */
  pipeline: SavePipeline;
  /** Persist a trimmed name; settles when the write settles. */
  onSave: (name: string) => Promise<unknown>;
}

export function IssueTitleField({ value, disabled, pipeline, onSave }: IssueTitleFieldProps): React.ReactElement {
  const [text, setText] = React.useState(value);
  const [focused, setFocused] = React.useState(false);
  const [dirty, setDirty] = React.useState(false);
  const stateRef = React.useRef({ text: value, dirty: false });
  stateRef.current = { text, dirty };
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

  // A server value arriving mid-edit wins: palette and layout edits
  // rename through the same cache, and the field tracks the cache.
  React.useEffect(() => {
    clearTimer();
    setText(value);
    setDirty(false);
  }, [value]);

  const flush = React.useCallback((serverValue: string): void => {
    clearTimer();
    const { text: current, dirty: wasDirty } = stateRef.current;
    if (!wasDirty) return;
    const trimmed = current.trim();
    if (trimmed.length === 0) {
      stateRef.current = { text: serverValue, dirty: false };
      setText(serverValue);
      setDirty(false);
      pipelineRef.current.markSaved();
      return;
    }
    stateRef.current = { text: trimmed, dirty: false };
    setText(trimmed);
    setDirty(false);
    if (trimmed === serverValue) {
      pipelineRef.current.markSaved();
      return;
    }
    pipelineRef.current.markSaving();
    void saveRef.current(trimmed).then(
      () => pipelineRef.current.markSaved(),
      () => pipelineRef.current.markSaved()
    );
  }, []);

  // Flush-on-unmount saves edits the debounce never fired for. The
  // settle handlers above swallow rejections, so nothing escapes.
  const flushRef = React.useRef(flush);
  flushRef.current = flush;
  const valueRef = React.useRef(value);
  valueRef.current = value;
  React.useEffect(
    () => () => {
      flushRef.current(valueRef.current);
    },
    []
  );

  if (disabled) {
    return <h1 className="text-h2 break-words text-(--text)">{value}</h1>;
  }

  return (
    <div className="flex flex-col gap-(--space-2)">
      <div className="relative">
        <textarea
          aria-label={TITLE_LABEL}
          aria-invalid={text.length === 0 ? true : undefined}
          className="text-h2 block w-full resize-none overflow-hidden rounded-(--radius-control) bg-transparent text-(--text) outline-none placeholder:text-(--text-muted) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1"
          style={{ fieldSizing: "content" }}
          value={text}
          maxLength={TITLE_MAX_LENGTH}
          placeholder={TITLE_PLACEHOLDER}
          rows={1}
          onChange={(event: React.ChangeEvent<HTMLTextAreaElement>) => {
            const next = event.target.value;
            setText(next);
            setDirty(true);
            pipeline.markSaving();
            clearTimer();
            timerRef.current = setTimeout(() => flush(valueRef.current), TITLE_AUTOSAVE_MS);
          }}
          onFocus={() => setFocused(true)}
          onBlur={() => {
            setFocused(false);
            flush(valueRef.current);
          }}
        />
        {focused ? (
          <p
            aria-hidden="true"
            className={`text-meta pointer-events-none absolute right-1 bottom-1 rounded-(--radius-control) bg-(--bg) px-(--space-2) ${
              text.length === 0 ? "text-(--danger)" : "text-(--text-muted)"
            }`}
          >
            {text.length}/{TITLE_MAX_LENGTH}
          </p>
        ) : null}
      </div>
      {text.length === 0 ? <p className="text-body font-medium text-(--danger)">{TITLE_REQUIRED_MESSAGE}</p> : null}
    </div>
  );
}
