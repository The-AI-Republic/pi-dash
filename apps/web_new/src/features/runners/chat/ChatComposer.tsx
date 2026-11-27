// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat composer (NEWFRONT-549 RUN-032, messaging NEWFRONT-551 RUN-027/030).
// One draft box that sends on an unmodified Enter and keeps the newline on
// Shift+Enter, a send control that stays shut for an empty draft, a
// sending lockout, a stop control while the agent replies, and the
// push-to-talk dictation control wherever dictation is set up. One bordered
// box holds the draft on top and a toolbar underneath: page-supplied
// controls (the desktop approval mode) on the left, mic and send/stop on
// the right. The page decides what the composer may do; this component
// renders it.

import { Mic, Send, Square } from "lucide-react";
import * as React from "react";

import { Button, cn } from "@pidash/kit";

import { appendDictation, useDictation } from "../../../shared/dictation/index.js";

export const CHAT_COMPOSER_PLACEHOLDER = "Message this runner…";
export const CHAT_SEND_LABEL = "Send message";
export const CHAT_STOP_LABEL = "Stop response";
export const CHAT_MIC_LABEL = "Hold to dictate";
export const CHAT_MIC_RECORDING_LABEL = "Release to transcribe";

export interface ChatComposerProps {
  /** Blocking reason; null means chat can proceed. */
  blockedReason: string | null;
  /** The draft the page holds; the composer is controlled. */
  draft: string;
  onDraftChange: (value: string) => void;
  /** Sends the trimmed draft; the page owns clearing and restoring it. */
  onSend: (draft: string) => void;
  /** A reply is in flight: the send control reads as stop. */
  turnInFlight: boolean;
  onStop: () => void;
  /** A send is in flight: nothing else may be typed or sent. */
  sending?: boolean;
  /** Offer the microphone control (dictation configured and enabled). */
  dictationAvailable?: boolean;
  /** Controls for the toolbar's leading edge, inside the box. */
  toolbar?: React.ReactNode;
}

/** Whole seconds as minutes and seconds. */
export function formatDictationElapsed(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

export function ChatComposer({
  blockedReason,
  draft,
  onDraftChange,
  onSend,
  turnInFlight,
  onStop,
  sending = false,
  dictationAvailable = false,
  toolbar,
}: ChatComposerProps): React.ReactElement {
  const blocked = blockedReason !== null;
  const locked = blocked || sending;
  const canSend = !blocked && !sending && !turnInFlight && draft.trim() !== "";

  const dictation = useDictation({ onResult: (text) => onDraftChange(appendDictation(draft, text)) });
  const showMic = dictationAvailable && dictation.supported;
  const recording = dictation.status === "recording" || dictation.status === "requesting";
  const transcribing = dictation.status === "transcribing";

  const submit = (): void => {
    if (!canSend) return;
    onSend(draft);
  };

  const hint =
    dictation.status === "denied"
      ? "Microphone access is blocked. Allow it in the browser's site settings, then try again."
      : dictation.status === "error"
        ? (dictation.errorMessage ?? "Voice input failed")
        : null;

  return (
    <div data-testid="runner-chat-composer" className="shrink-0 pt-(--space-3)">
      {blocked ? (
        <div data-testid="runner-chat-block-reason" className="text-meta mb-(--space-2) text-(--text-muted)">
          {blockedReason}
        </div>
      ) : null}
      {recording ? (
        <div data-testid="runner-chat-dictation-hint" className="text-meta mb-(--space-2) text-(--danger)">
          Recording — release to transcribe · {formatDictationElapsed(dictation.elapsedSeconds)}
        </div>
      ) : transcribing ? (
        <div data-testid="runner-chat-dictation-hint" className="text-meta mb-(--space-2) text-(--text-muted)">
          Transcribing…
        </div>
      ) : hint === null ? null : (
        <div data-testid="runner-chat-dictation-hint" className="text-meta mb-(--space-2) text-(--danger)">
          {hint}
        </div>
      )}
      <div
        data-testid="runner-chat-composer-box"
        className={cn(
          "flex flex-col gap-(--space-2) rounded-(--radius-dialog) border border-(--border) bg-(--surface)",
          "px-(--space-4) pt-(--space-3) pb-(--space-3) transition-colors focus-within:border-(--accent)",
          locked && "opacity-60"
        )}
      >
        <textarea
          aria-label="Message"
          placeholder={CHAT_COMPOSER_PLACEHOLDER}
          rows={3}
          value={draft}
          disabled={locked}
          onChange={(event) => onDraftChange(event.target.value)}
          onKeyDown={(event) => {
            // Only an unmodified Enter sends; Shift+Enter keeps the canvas
            // newline and any chord is left to the browser.
            if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) return;
            event.preventDefault();
            submit();
          }}
          className="text-body min-h-16 w-full resize-none border-0 bg-transparent px-(--space-1) text-(--text) placeholder:text-(--text-muted) focus:outline-none disabled:cursor-not-allowed"
        />
        <div className="flex items-center gap-(--space-2)">
          {toolbar}
          <div className="ml-auto flex items-center gap-(--space-2)">
            {showMic ? (
              <Button
                variant={recording ? "danger" : "ghost"}
                size="small"
                aria-label={recording ? CHAT_MIC_RECORDING_LABEL : CHAT_MIC_LABEL}
                aria-pressed={dictation.status === "recording"}
                disabled={blocked || transcribing}
                loading={transcribing}
                title={recording ? CHAT_MIC_RECORDING_LABEL : CHAT_MIC_LABEL}
                onPointerDown={(event) => {
                  event.preventDefault();
                  dictation.reset();
                  void dictation.start();
                }}
                onPointerUp={() => dictation.stop()}
                onPointerLeave={() => {
                  if (dictation.status === "recording") dictation.stop();
                }}
                onPointerCancel={() => dictation.cancel()}
                onKeyDown={(event) => {
                  if ((event.key !== " " && event.key !== "Enter") || event.repeat) return;
                  event.preventDefault();
                  dictation.reset();
                  void dictation.start();
                }}
                onKeyUp={(event) => {
                  if (event.key !== " " && event.key !== "Enter") return;
                  event.preventDefault();
                  dictation.stop();
                }}
                className="tabular-nums"
              >
                {dictation.status === "recording" ? (
                  <span className="flex items-center gap-(--space-1)">
                    <Mic size={16} strokeWidth={1.5} aria-hidden="true" />
                    {formatDictationElapsed(dictation.elapsedSeconds)}
                  </span>
                ) : (
                  <Mic size={16} strokeWidth={1.5} aria-hidden="true" />
                )}
              </Button>
            ) : null}
            {turnInFlight ? (
              <Button
                variant="danger"
                size="small"
                aria-label={CHAT_STOP_LABEL}
                title={CHAT_STOP_LABEL}
                onClick={onStop}
              >
                <Square size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            ) : (
              <Button
                aria-label={CHAT_SEND_LABEL}
                size="small"
                variant="primary"
                disabled={!canSend}
                loading={sending}
                onClick={submit}
              >
                <Send size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
