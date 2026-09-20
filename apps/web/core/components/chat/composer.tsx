/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useRef, type ReactNode } from "react";
import { Mic, Send, Square } from "lucide-react";
import { Link, useNavigate } from "react-router";
import { Button } from "@pi-dash/ui";
import { cn } from "@pi-dash/utils";
import { DICTATION_SETTINGS_ANCHOR } from "@/components/settings/profile/content/pages/dictation-settings";
import { useDictation } from "@/hooks/use-dictation";

interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: () => void;
  onStop?: () => void;
  busy?: boolean;
  sending?: boolean;
  disabledReason?: string | null;
  placeholder?: string;
  /** Docked composers (bottom of a chat) draw a top border; standalone ones don't. */
  bordered?: boolean;
}

const DICTATION_SETTINGS_HREF = `/settings/profile/ai-assistant#${DICTATION_SETTINGS_ANCHOR}`;

function formatElapsed(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

/** Append transcribed text to the draft, inserting a space when the draft
 * already ends in a word so dictation adds to — rather than mangles — text. */
function appendToDraft(current: string, addition: string): string {
  if (!current) return addition;
  return /\s$/.test(current) ? `${current}${addition}` : `${current} ${addition}`;
}

export function ChatComposer({
  draft,
  onDraftChange,
  onSend,
  onStop,
  busy = false,
  sending = false,
  disabledReason = null,
  placeholder = "Ask Pi Dash to do something…",
  bordered = true,
}: ChatComposerProps) {
  const disabled = !!disabledReason;
  const navigate = useNavigate();

  // Keep the latest draft reachable from the (stable) onResult callback so the
  // transcript appends to whatever is in the box when recording finishes.
  const draftRef = useRef(draft);
  draftRef.current = draft;

  const handleTranscript = useCallback(
    (text: string) => onDraftChange(appendToDraft(draftRef.current, text)),
    [onDraftChange]
  );

  const dictation = useDictation({ onResult: handleTranscript });

  const {
    status: dictationStatus,
    elapsedMs,
    errorMessage,
    isUnconfigured,
    isSupported: dictationSupported,
    start: startDictation,
    stop: stopDictation,
    cancel: cancelDictation,
    reset: resetDictation,
  } = dictation;

  const beginDictation = useCallback(() => {
    if (disabled) return;
    // Not set up yet: route to settings instead of a silent no-op recording.
    if (isUnconfigured) {
      navigate(DICTATION_SETTINGS_HREF);
      return;
    }
    // A prior error / denied press should retry cleanly.
    if (dictationStatus === "error" || dictationStatus === "denied") resetDictation();
    void startDictation();
  }, [disabled, isUnconfigured, dictationStatus, navigate, resetDictation, startDictation]);

  const endDictation = useCallback(() => {
    if (dictationStatus === "recording" || dictationStatus === "requesting") stopDictation();
  }, [dictationStatus, stopDictation]);

  const isRecording = dictationStatus === "recording";
  const isTranscribing = dictationStatus === "transcribing";

  const micLabel = isUnconfigured
    ? "Set up voice dictation"
    : isRecording
      ? "Release to transcribe"
      : "Hold to dictate";

  // A single-line status/hint shown above the input for the non-idle states.
  let dictationHint: ReactNode = null;
  if (isRecording) {
    dictationHint = (
      <span className="text-danger flex items-center gap-1.5">
        <span className="bg-danger size-2 animate-pulse rounded-full" aria-hidden />
        Recording — release to transcribe · {formatElapsed(elapsedMs)}
      </span>
    );
  } else if (isTranscribing) {
    dictationHint = <span className="text-secondary">Transcribing…</span>;
  } else if (dictationStatus === "denied") {
    dictationHint = (
      <span className="text-danger">
        Microphone access is blocked. Enable it in your browser&apos;s site settings, then try again.
      </span>
    );
  } else if (dictationStatus === "error") {
    dictationHint = <span className="text-danger">{errorMessage ?? "Voice input failed. Try again."}</span>;
  } else if (isUnconfigured) {
    dictationHint = (
      <span className="text-secondary">
        Voice dictation isn&apos;t set up.{" "}
        <Link to={DICTATION_SETTINGS_HREF} className="text-accent-strong underline underline-offset-2">
          Configure it
        </Link>
        .
      </span>
    );
  }

  return (
    <div className={cn("shrink-0", bordered && "border-t border-subtle pt-3")}>
      {disabledReason && <div className="mb-2 text-12 text-secondary">{disabledReason}</div>}
      {dictationHint && <div className="mb-2 text-12">{dictationHint}</div>}
      <div className="flex items-end gap-2">
        <textarea
          value={draft}
          onChange={(e) => onDraftChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              if (!disabled && draft.trim()) onSend();
            }
          }}
          disabled={disabled || sending}
          placeholder={placeholder}
          className="min-h-20 flex-1 resize-none rounded-md border border-subtle bg-surface-1 px-3 py-2 text-13 outline-none focus:border-accent-strong"
        />
        {dictationSupported && (
          <Button
            variant={isRecording ? "accent-danger" : "neutral-primary"}
            aria-label={micLabel}
            aria-pressed={isRecording}
            title={micLabel}
            disabled={disabled || isTranscribing}
            loading={isTranscribing}
            // Push-to-talk: hold (pointer or Space/Enter) to record, release to
            // transcribe. onClick is intentionally not used so a keyboard press
            // maps to the same down/up hold gesture as the mouse.
            onPointerDown={(e) => {
              e.preventDefault();
              beginDictation();
            }}
            onPointerUp={endDictation}
            onPointerLeave={() => {
              if (isRecording) endDictation();
            }}
            onPointerCancel={() => cancelDictation()}
            onKeyDown={(e) => {
              if ((e.key === " " || e.key === "Enter") && !e.repeat) {
                e.preventDefault();
                beginDictation();
              }
            }}
            onKeyUp={(e) => {
              if (e.key === " " || e.key === "Enter") {
                e.preventDefault();
                endDictation();
              }
            }}
            onContextMenu={(e) => e.preventDefault()}
          >
            {isRecording ? (
              <span className="flex items-center gap-1 tabular-nums">
                <Mic className="size-4" />
                {formatElapsed(elapsedMs)}
              </span>
            ) : (
              <Mic className="size-4" />
            )}
          </Button>
        )}
        {busy && onStop ? (
          <Button onClick={onStop} variant="tertiary-danger">
            <Square className="size-4" />
          </Button>
        ) : (
          <Button onClick={onSend} disabled={disabled || !draft.trim()} loading={sending}>
            <Send className="size-4" />
          </Button>
        )}
      </div>
    </div>
  );
}
