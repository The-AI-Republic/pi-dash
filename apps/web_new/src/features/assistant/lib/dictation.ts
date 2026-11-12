// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dictation cell rules (parity/specs/agents.md §12, AGT-050/051/057/064).
// Pure decisions behind the composer's microphone: the kill-switch gate,
// the label, and the hint line for every state. The setup hint renders
// its own settings link; every other kind renders plain text.

import type { DictationCaptureFailure, DictationHandle, DictationStatus } from "../../../shared/dictation/index.js";

import {
  MIC_DENIED_HINT,
  MIC_FAILED_FALLBACK,
  MIC_FAILED_HINT,
  MIC_MISSING_HINT,
  MIC_READY_LABEL,
  MIC_RECORDING_HINT,
  MIC_RECORDING_LABEL,
  MIC_SETUP_LABEL,
  MIC_TRANSCRIBING_HINT,
} from "./copy.js";

/** The speech read the cell decides on (a subset of the stored config). */
export interface DictationSpeechState {
  enabled?: boolean | undefined;
  has_api_key: boolean;
}

/** Everything the composer needs to offer dictation: the read plus the live capture handle. */
export interface ComposerDictation {
  speech: DictationSpeechState | undefined;
  handle: DictationHandle;
}

/**
 * True when the microphone renders: the browser can capture and the switch
 * was not turned off. The switch travels on the speech read, so an absent
 * flag is a backend that predates it, not a disabled one, and the cell stays
 * on (AGT-050/AGT-057); only an explicit off hides it (AGT-064). A read
 * still loading or failed has nothing to decide on and hides it, and a
 * missing key never hides it — the keyless press routes to setup (AGT-051).
 */
export function isDictationCellShown(speech: DictationSpeechState | undefined, supported: boolean): boolean {
  return supported && speech !== undefined && speech.enabled !== false;
}

/** The microphone's accessible label from the key and recording state. */
export function dictationMicLabel(hasKey: boolean, recording: boolean): string {
  if (!hasKey) return MIC_SETUP_LABEL;
  return recording ? MIC_RECORDING_LABEL : MIC_READY_LABEL;
}

/** Whole seconds as minutes and seconds. */
export function formatDictationElapsed(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

/** The hint above the composer, or null when nothing shows. */
export type DictationHint =
  | { kind: "recording"; seconds: number }
  | { kind: "transcribing" }
  | { kind: "denied" }
  | { kind: "capture"; failure: DictationCaptureFailure | null; message: string | null }
  | { kind: "setup" };

/** The hint for a capture state; idle asks for setup only without a key. */
export function dictationHint(input: {
  status: DictationStatus;
  captureFailure: DictationCaptureFailure | null;
  errorMessage: string | null;
  hasKey: boolean;
  elapsedSeconds: number;
}): DictationHint | null {
  switch (input.status) {
    case "recording":
      return { kind: "recording", seconds: input.elapsedSeconds };
    case "transcribing":
      return { kind: "transcribing" };
    case "denied":
      return { kind: "denied" };
    case "error":
      return { kind: "capture", failure: input.captureFailure, message: input.errorMessage };
    default:
      return input.hasKey ? null : { kind: "setup" };
  }
}

/** The hint line for the kinds that render as plain text. */
export function dictationHintText(hint: Exclude<DictationHint, { kind: "setup" }>): string {
  switch (hint.kind) {
    case "recording":
      return `${MIC_RECORDING_HINT} · ${formatDictationElapsed(hint.seconds)}`;
    case "transcribing":
      return MIC_TRANSCRIBING_HINT;
    case "denied":
      return MIC_DENIED_HINT;
    case "capture":
      if (hint.failure === "missing-hardware") return MIC_MISSING_HINT;
      if (hint.failure === "capture-failed") return MIC_FAILED_HINT;
      return hint.message ?? MIC_FAILED_FALLBACK;
  }
}
