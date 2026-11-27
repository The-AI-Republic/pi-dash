// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer shell (parity/specs/agents.md §9, AGT-038/AGT-040). One
// multiline draft box with a send action; the thread slice adds the
// streaming stop state and the keys slice the voice cell
// (AGT-050/051/057/064). Without a provider key the
// whole input locks and the reminder line explains why, so a submit can
// never reach the server. Submits go through the form, which makes the
// send button's `type="submit"` and an unmodified Enter the same path.

import { Button, cn } from "@pidash/kit";
import { Mic, Send, Square } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { ChatInput } from "../../../shared/editor/ChatInput.js";
import type { ComposerDictation } from "../lib/dictation.js";
import {
  API_KEY_REMINDER,
  ASSISTANT_COMPOSER_LABEL,
  ASSISTANT_PLACEHOLDER,
  ASSISTANT_SEND_LABEL,
  ASSISTANT_STOP_LABEL,
  MIC_SETUP_ACTION,
  MIC_SETUP_HINT,
  providerSettingsHref,
  VOICE_DICTATION_HASH,
} from "../lib/copy.js";
import {
  dictationHint,
  dictationHintText,
  dictationMicLabel,
  formatDictationElapsed,
  isDictationCellShown,
} from "../lib/dictation.js";

export interface ComposerProps {
  /**
   * Workspace whose account settings hold the provider key (main#670);
   * the dictation and keyless-setup links land inside it.
   */
  workspaceSlug: string;
  draft: string;
  onDraftChange: (value: string) => void;
  onSubmit: (draft: string) => void;
  /** A start or send is in flight: the box locks so a second submit cannot fire. */
  starting?: boolean;
  /** A send is posting (the loading state of the send action). */
  posting?: boolean;
  /** A turn is running: stop replaces send and nothing can be submitted. */
  running?: boolean;
  /** Cancels the running turn; the box stays locked until the server confirms. */
  onStop?: () => void;
  /** False only when no provider key exists. */
  configured?: boolean;
  placeholder?: string;
  /** Standalone composers draw no top rule; docked ones do. */
  bordered?: boolean;
  className?: string;
  /** Voice dictation cell; absent on surfaces without a microphone. */
  dictation?: ComposerDictation | undefined;
  /** Model picker rendered in the bottom bar next to send; absent elsewhere. */
  modelPicker?: React.ReactNode;
}

/** Caps the draft box's growth; beyond this the draft scrolls inside it. */
const COMPOSER_MAX_HEIGHT = 240;

export function Composer({
  workspaceSlug,
  draft,
  onDraftChange,
  onSubmit,
  starting = false,
  posting = false,
  running = false,
  onStop,
  configured = true,
  placeholder = ASSISTANT_PLACEHOLDER,
  bordered = true,
  className,
  dictation,
  modelPicker,
}: ComposerProps): React.ReactElement {
  // A running turn does not disable the box: the draft stays editable and an
  // unmodified Enter still posts, exactly as the old surface does. The server
  // owns the race and answers `turn_active`, which the thread shows inline;
  // the visible action is what swaps to stop, so no second send can come from
  // a button while the turn runs (AGT-042/AGT-043/AGT-049).
  const canSend = configured && !starting && !posting && draft.trim() !== "";
  const boxLocked = !configured || starting || posting;

  const handle = dictation?.handle;
  const micShown = dictation !== undefined && isDictationCellShown(dictation.speech, dictation.handle.supported);
  React.useEffect(() => {
    // A hidden cell abandons an in-progress capture: the switch turned
    // off mid-recording, so the utterance must not upload (AGT-064).
    if (handle === undefined || micShown) return;
    if (handle.status === "requesting" || handle.status === "recording") handle.cancel();
  }, [handle, micShown]);

  const settingsHref = providerSettingsHref(workspaceSlug);
  const hasSpeechKey = dictation?.speech?.has_api_key === true;
  const recording = handle?.status === "recording";
  const transcribing = handle?.status === "transcribing";
  const micLabel = dictationMicLabel(hasSpeechKey, recording);
  const micDisabled = boxLocked || transcribing;
  const hint =
    dictation === undefined || !micShown
      ? null
      : dictationHint({
          status: dictation.handle.status,
          captureFailure: dictation.handle.captureFailure,
          errorMessage: dictation.handle.errorMessage,
          hasKey: hasSpeechKey,
          elapsedSeconds: dictation.handle.elapsedSeconds,
        });

  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (!canSend) return;
    onSubmit(draft.trim());
  };

  return (
    <form
      onSubmit={submit}
      data-testid="assistant-composer"
      data-locked={String(!configured)}
      className={cn(
        "flex shrink-0 flex-col gap-(--space-2)",
        bordered && "border-t border-(--border) pt-(--space-3)",
        className
      )}
    >
      {configured ? null : (
        <p data-testid="assistant-composer-reason" className="text-meta text-(--text-muted)">
          {API_KEY_REMINDER}
        </p>
      )}
      {hint === null ? null : hint.kind === "setup" ? (
        <p data-testid="assistant-dictation-hint" className="text-meta text-(--text-muted)">
          {MIC_SETUP_HINT}{" "}
          <CrossAppLink to={settingsHref} hash={VOICE_DICTATION_HASH} className="underline underline-offset-2">
            {MIC_SETUP_ACTION}
          </CrossAppLink>
          .
        </p>
      ) : (
        <p
          data-testid="assistant-dictation-hint"
          className={cn("text-meta", hint.kind === "transcribing" ? "text-(--text-muted)" : "text-(--danger)")}
        >
          {dictationHintText(hint)}
        </p>
      )}
      {/* One box holds the draft and its controls: the editor on top, the
          model picker bottom-left, dictation and send/stop bottom-right. */}
      <div
        data-testid="assistant-composer-box"
        className={cn(
          "flex flex-col gap-(--space-2) rounded-(--radius-dialog) border border-(--border) bg-(--surface)",
          "px-(--space-4) pt-(--space-4) pb-(--space-3) transition-colors focus-within:border-(--accent)"
        )}
      >
        {/* Only an unmodified Enter submits: Shift+Enter breaks the line
            and a chord never fires a send (AGT-049 intended). A running
            turn is not a client-side gate; the send still posts and the
            server's `turn_active` reply lands inline. */}
        <ChatInput
          label={ASSISTANT_COMPOSER_LABEL}
          placeholder={placeholder}
          value={draft}
          disabled={boxLocked}
          onChange={onDraftChange}
          onSubmit={() => {
            if (canSend) onSubmit(draft.trim());
          }}
          maxHeight={COMPOSER_MAX_HEIGHT}
          className="min-h-12 px-(--space-2)"
        />
        <div className="flex items-center gap-(--space-2)">
          {modelPicker}
          <div className="ml-auto flex items-center gap-(--space-2)">
            {dictation !== undefined && micShown ? (
              hasSpeechKey || micDisabled ? (
                <Button
                  variant={recording ? "danger" : "ghost"}
                  size="small"
                  data-testid="assistant-mic"
                  aria-label={micLabel}
                  aria-pressed={recording}
                  title={micLabel}
                  disabled={micDisabled}
                  loading={transcribing}
                  // Push-to-talk: hold (pointer or Space/Enter) to record,
                  // release to transcribe. onClick stays off so a keyboard
                  // press maps to the same hold gesture as the pointer.
                  onPointerDown={(event) => {
                    event.preventDefault();
                    dictation.handle.reset();
                    void dictation.handle.start();
                  }}
                  onPointerUp={() => dictation.handle.stop()}
                  onPointerLeave={() => {
                    if (dictation.handle.status === "recording") dictation.handle.stop();
                  }}
                  onPointerCancel={() => dictation.handle.cancel()}
                  onKeyDown={(event) => {
                    if ((event.key !== " " && event.key !== "Enter") || event.repeat) return;
                    event.preventDefault();
                    dictation.handle.reset();
                    void dictation.handle.start();
                  }}
                  onKeyUp={(event) => {
                    if (event.key !== " " && event.key !== "Enter") return;
                    event.preventDefault();
                    dictation.handle.stop();
                  }}
                  className="tabular-nums"
                >
                  {recording ? (
                    <span className="flex items-center gap-(--space-1)">
                      <Mic size={16} strokeWidth={1.5} aria-hidden="true" />
                      {formatDictationElapsed(dictation.handle.elapsedSeconds)}
                    </span>
                  ) : (
                    <Mic size={16} strokeWidth={1.5} aria-hidden="true" />
                  )}
                </Button>
              ) : (
                // No speech key: the press routes to dictation settings
                // instead of recording (AGT-051).
                <CrossAppLink
                  to={settingsHref}
                  hash={VOICE_DICTATION_HASH}
                  data-testid="assistant-mic"
                  aria-label={micLabel}
                  title={micLabel}
                  className="text-meta inline-flex h-6 cursor-pointer items-center justify-center gap-(--space-2) rounded-(--radius-control) bg-transparent px-(--space-3) font-medium whitespace-nowrap text-(--text) transition-colors duration-(--motion-duration) hover:bg-(--subtle) focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--focus-ring-color)"
                >
                  <Mic size={16} strokeWidth={1.5} aria-hidden="true" />
                </CrossAppLink>
              )
            ) : null}
            {running ? (
              <Button
                variant="secondary"
                aria-label={ASSISTANT_STOP_LABEL}
                onClick={(event) => {
                  event.preventDefault();
                  onStop?.();
                }}
              >
                <Square aria-hidden="true" size={16} />
              </Button>
            ) : (
              <Button
                type="submit"
                variant="primary"
                disabled={!canSend}
                loading={starting || posting}
                aria-label={ASSISTANT_SEND_LABEL}
              >
                <Send aria-hidden="true" size={16} />
              </Button>
            )}
          </div>
        </div>
      </div>
    </form>
  );
}
