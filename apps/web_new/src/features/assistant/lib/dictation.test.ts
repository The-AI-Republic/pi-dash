// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dictation cell rules (AGT-050/051/057/064): the mic shows only while
// the kill switch reads on and capture exists, labels follow the key and
// recording state, and every hint kind maps to its line.
import { describe, expect, it } from "vitest";

import type { DictationCaptureFailure, DictationStatus } from "../../../shared/dictation/index.js";
import {
  dictationHint,
  dictationHintText,
  dictationMicLabel,
  formatDictationElapsed,
  isDictationCellShown,
} from "./dictation.js";

describe("isDictationCellShown", () => {
  it("treats an absent switch as a pre-switch backend and shows the mic", () => {
    // The parity backend predates the kill switch: its speech read carries a
    // key and no `enabled` field, and the composer must still offer dictation.
    expect(isDictationCellShown({ has_api_key: true }, true)).toBe(true);
    expect(isDictationCellShown({ enabled: true, has_api_key: true }, true)).toBe(true);
    // A keyless read keeps the mic so the press can route to setup.
    expect(isDictationCellShown({ enabled: true, has_api_key: false }, true)).toBe(true);
    expect(isDictationCellShown({ has_api_key: false }, true)).toBe(true);
  });

  it("hides the mic on an explicit off, an unread config or no capture", () => {
    expect(isDictationCellShown({ enabled: false, has_api_key: true }, true)).toBe(false);
    expect(isDictationCellShown({ enabled: false, has_api_key: false }, true)).toBe(false);
    expect(isDictationCellShown(undefined, true)).toBe(false);
    expect(isDictationCellShown({ has_api_key: true }, false)).toBe(false);
    expect(isDictationCellShown({ enabled: true, has_api_key: true }, false)).toBe(false);
  });
});

describe("dictationMicLabel", () => {
  it("names setup without a key and the hold state with one", () => {
    expect(dictationMicLabel(false, false)).toBe("Set up voice dictation");
    expect(dictationMicLabel(false, true)).toBe("Set up voice dictation");
    expect(dictationMicLabel(true, false)).toBe("Hold to dictate");
    expect(dictationMicLabel(true, true)).toBe("Release to transcribe");
  });
});

describe("formatDictationElapsed", () => {
  it("reads whole seconds as minutes and seconds", () => {
    expect(formatDictationElapsed(0)).toBe("0:00");
    expect(formatDictationElapsed(7)).toBe("0:07");
    expect(formatDictationElapsed(75)).toBe("1:15");
  });
});

describe("dictationHint", () => {
  const base = {
    captureFailure: null as DictationCaptureFailure | null,
    errorMessage: null as string | null,
    hasKey: true,
    elapsedSeconds: 0,
  };

  it("reports recording, upload and denial by status", () => {
    expect(dictationHint({ ...base, status: "recording", elapsedSeconds: 9 })).toEqual({
      kind: "recording",
      seconds: 9,
    });
    expect(dictationHint({ ...base, status: "transcribing" })).toEqual({ kind: "transcribing" });
    expect(dictationHint({ ...base, status: "denied" })).toEqual({ kind: "denied" });
  });

  it("carries the failure kind and message for capture and upload errors", () => {
    expect(dictationHint({ ...base, status: "error", captureFailure: "missing-hardware" })).toEqual({
      kind: "capture",
      failure: "missing-hardware",
      message: null,
    });
    expect(dictationHint({ ...base, status: "error", errorMessage: "provider down" })).toEqual({
      kind: "capture",
      failure: null,
      message: "provider down",
    });
  });

  it("asks for setup only when idle without a key", () => {
    const idle: DictationStatus = "idle";
    const requesting: DictationStatus = "requesting";
    expect(dictationHint({ ...base, status: idle, hasKey: false })).toEqual({ kind: "setup" });
    expect(dictationHint({ ...base, status: idle, hasKey: true })).toBeNull();
    expect(dictationHint({ ...base, status: requesting, hasKey: true })).toBeNull();
  });
});

describe("dictationHintText", () => {
  it("renders the recording line with its readout", () => {
    expect(dictationHintText({ kind: "recording", seconds: 65 })).toBe("Recording — release to transcribe · 1:05");
  });

  it("renders the upload and denial lines", () => {
    expect(dictationHintText({ kind: "transcribing" })).toBe("Transcribing…");
    expect(dictationHintText({ kind: "denied" })).toBe(
      "Microphone access is blocked. Enable it in your browser's site settings, then try again."
    );
  });

  it("names the capture failure before the raw message", () => {
    expect(dictationHintText({ kind: "capture", failure: "missing-hardware", message: "x" })).toBe(
      "No microphone found."
    );
    expect(dictationHintText({ kind: "capture", failure: "capture-failed", message: "x" })).toBe(
      "Couldn't access the microphone."
    );
    expect(dictationHintText({ kind: "capture", failure: null, message: "provider down" })).toBe("provider down");
    expect(dictationHintText({ kind: "capture", failure: null, message: null })).toBe("Voice input failed");
  });
});
