/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import useSWR from "swr";
import { AssistantService } from "@pi-dash/services";
import type { IUserSTTConfig } from "@pi-dash/types";

/**
 * Push-to-talk voice dictation for the chat composer. Wraps the browser
 * MediaRecorder API: hold to record, release to transcribe. The recorded
 * webm/opus blob is POSTed to the user's configured STT endpoint and the
 * returned text is handed back through {@link UseDictationOptions.onResult}
 * for the composer to append to its draft.
 *
 * There is no getUserMedia / MediaRecorder anywhere else in the web app — this
 * is the first (and only) audio-capture surface, so the whole lifecycle
 * (permission, stream cleanup, recording cap) is owned here.
 */

/** Distinct UI states the mic button renders. `denied` and `unconfigured` are
 * deliberately separate from `error`: the browser permission prompt appears
 * once, so a user who dismissed it needs a specific hint; and an unconfigured
 * endpoint should route to settings rather than read as a failure. */
export type DictationStatus =
  | "idle"
  | "requesting"
  | "recording"
  | "transcribing"
  | "error"
  | "denied"
  | "unconfigured";

export interface UseDictationOptions {
  /** Called with the transcribed text once recording is transcribed. */
  onResult: (text: string) => void;
}

export interface UseDictation {
  status: DictationStatus;
  /** Milliseconds elapsed in the current recording (0 when not recording). */
  elapsedMs: number;
  /** Human-readable message for the `error` state. */
  errorMessage: string | null;
  /** True once we know the endpoint is not configured (routes to settings). */
  isUnconfigured: boolean;
  /** True while recording. */
  isRecording: boolean;
  /** Whether the browser exposes MediaRecorder + getUserMedia at all. */
  isSupported: boolean;
  /** Fraction (0..1) of the recording cap consumed, for a progress affordance. */
  capFraction: number;
  /** Begin capturing (push-to-talk down). No-op if already active. */
  start: () => Promise<void>;
  /** Stop capturing and transcribe (push-to-talk up). */
  stop: () => void;
  /** Abandon the current recording without transcribing. */
  cancel: () => void;
  /** Clear an error / unconfigured state back to idle. */
  reset: () => void;
}

/** Cap a single utterance so the upload stays well under provider limits
 * (OpenAI's transcription cap is 25 MB; opus at ~24 kbps is ~180 KB for 60 s). */
const MAX_RECORDING_MS = 60_000;
/** Ignore accidental taps: a sub-half-second press yields no useful audio. */
const MIN_RECORDING_MS = 500;
const TICK_MS = 200;

const service = new AssistantService();

function pickMimeType(): string {
  if (typeof MediaRecorder === "undefined" || typeof MediaRecorder.isTypeSupported !== "function") return "";
  const candidates = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus"];
  return candidates.find((t) => MediaRecorder.isTypeSupported(t)) ?? "";
}

export function useDictation({ onResult }: UseDictationOptions): UseDictation {
  const isSupported =
    typeof navigator !== "undefined" &&
    !!navigator.mediaDevices &&
    typeof navigator.mediaDevices.getUserMedia === "function" &&
    typeof MediaRecorder !== "undefined";

  const [status, setStatus] = useState<DictationStatus>("idle");
  const [elapsedMs, setElapsedMs] = useState(0);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const streamRef = useRef<MediaStream | null>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const startedAtRef = useRef(0);
  const tickRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const capRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** When true, the recorder's onstop handler discards audio instead of sending. */
  const abortRef = useRef(false);
  /** Set when stop/cancel is requested during the getUserMedia await, before a
   * recorder exists; start() honours it as soon as recording begins so a fast
   * release can't leave the mic recording until the cap. */
  const pendingStopRef = useRef<null | "stop" | "cancel">(null);

  // Pre-check configuration via the shared SWR cache (same key the settings
  // page uses). Undefined while loading or if the endpoint is unavailable.
  const { data: config } = useSWR<IUserSTTConfig>("assistant-stt-config", () => service.getSTTConfig(), {
    shouldRetryOnError: false,
  });
  const knownUnconfigured = config ? !config.has_api_key : false;

  const clearTimers = useCallback(() => {
    if (tickRef.current) {
      clearInterval(tickRef.current);
      tickRef.current = null;
    }
    if (capRef.current) {
      clearTimeout(capRef.current);
      capRef.current = null;
    }
  }, []);

  // Stop and release the OS microphone. Leaving a track live keeps the OS mic
  // indicator lit, which reads as spyware — so this must run on every exit path.
  const releaseStream = useCallback(() => {
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
    recorderRef.current = null;
  }, []);

  const transcribe = useCallback(
    async (blob: Blob) => {
      setStatus("transcribing");
      try {
        const { text } = await service.transcribeAudio(blob);
        const trimmed = (text ?? "").trim();
        if (trimmed) onResult(trimmed);
        setStatus("idle");
      } catch (err: unknown) {
        const code = (err as { error_code?: string; code?: string } | null)?.error_code;
        if (code === "not_configured") {
          setStatus("unconfigured");
          return;
        }
        const detail = (err as { detail?: string; error?: string } | null)?.detail;
        setErrorMessage(detail || "Transcription failed. Try again.");
        setStatus("error");
      }
    },
    [onResult]
  );

  const stop = useCallback(() => {
    const recorder = recorderRef.current;
    if (!recorder || recorder.state === "inactive") {
      // Released before the recorder was ready (still awaiting getUserMedia):
      // remember it so start() stops the moment it begins.
      if (status === "requesting") pendingStopRef.current = "stop";
      return;
    }
    clearTimers();
    const heldFor = Date.now() - startedAtRef.current;
    // Too short to be intentional speech — discard rather than upload silence.
    abortRef.current = heldFor < MIN_RECORDING_MS;
    recorder.stop(); // fires onstop, which either transcribes or discards
  }, [clearTimers, status]);

  const cancel = useCallback(() => {
    abortRef.current = true;
    clearTimers();
    const recorder = recorderRef.current;
    if (recorder && recorder.state !== "inactive") {
      recorder.stop();
    } else {
      if (status === "requesting") pendingStopRef.current = "cancel";
      releaseStream();
    }
    setElapsedMs(0);
    setStatus("idle");
  }, [clearTimers, releaseStream, status]);

  const start = useCallback(async () => {
    if (!isSupported) {
      setErrorMessage("Voice input isn't supported in this browser.");
      setStatus("error");
      return;
    }
    if (status === "recording" || status === "requesting" || status === "transcribing") return;
    if (knownUnconfigured) {
      setStatus("unconfigured");
      return;
    }

    setErrorMessage(null);
    setStatus("requesting");
    abortRef.current = false;
    pendingStopRef.current = null;
    chunksRef.current = [];

    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (err: unknown) {
      const name = (err as { name?: string } | null)?.name;
      if (name === "NotAllowedError" || name === "SecurityError" || name === "PermissionDeniedError") {
        setStatus("denied");
      } else if (name === "NotFoundError" || name === "DevicesNotFoundError") {
        setErrorMessage("No microphone found.");
        setStatus("error");
      } else {
        setErrorMessage("Couldn't access the microphone.");
        setStatus("error");
      }
      return;
    }

    streamRef.current = stream;
    const mimeType = pickMimeType();
    const recorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined);
    recorderRef.current = recorder;

    recorder.ondataavailable = (e) => {
      if (e.data && e.data.size > 0) chunksRef.current.push(e.data);
    };
    recorder.onstop = () => {
      clearTimers();
      setElapsedMs(0);
      const chunks = chunksRef.current;
      chunksRef.current = [];
      const aborted = abortRef.current;
      releaseStream();
      if (aborted || chunks.length === 0) {
        // Discarded (accidental tap / cancel) — return to idle without a request.
        setStatus((prev) => (prev === "requesting" || prev === "recording" ? "idle" : prev));
        return;
      }
      const blob = new Blob(chunks, { type: mimeType || chunks[0]?.type || "audio/webm" });
      void transcribe(blob);
    };

    startedAtRef.current = Date.now();
    setElapsedMs(0);
    recorder.start();
    setStatus("recording");

    // Released (or cancelled) while we were still awaiting the mic — honour it
    // now that a recorder exists, so the stream can't stay live until the cap.
    const pending = pendingStopRef.current;
    pendingStopRef.current = null;
    if (pending === "cancel") {
      cancel();
      return;
    }
    if (pending === "stop") {
      stop();
      return;
    }

    tickRef.current = setInterval(() => {
      setElapsedMs(Date.now() - startedAtRef.current);
    }, TICK_MS);
    // Auto-stop at the cap so the upload stays under the size limit.
    capRef.current = setTimeout(() => stop(), MAX_RECORDING_MS);
  }, [isSupported, status, knownUnconfigured, clearTimers, releaseStream, transcribe, stop, cancel]);

  const reset = useCallback(() => {
    setErrorMessage(null);
    setElapsedMs(0);
    setStatus("idle");
  }, []);

  // Clean up on unmount / navigation: stop timers and release the mic so no
  // stream leaks past the composer's lifetime.
  useEffect(() => {
    return () => {
      clearTimers();
      const recorder = recorderRef.current;
      if (recorder && recorder.state !== "inactive") {
        abortRef.current = true;
        try {
          recorder.stop();
        } catch {
          // ignore — releaseStream below still tears the tracks down
        }
      }
      releaseStream();
    };
  }, [clearTimers, releaseStream]);

  return {
    status,
    elapsedMs,
    errorMessage,
    isUnconfigured: status === "unconfigured" || knownUnconfigured,
    isRecording: status === "recording",
    isSupported,
    capFraction: Math.min(elapsedMs / MAX_RECORDING_MS, 1),
    start,
    stop,
    cancel,
    reset,
  };
}
