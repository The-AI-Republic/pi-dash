// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Push-to-talk dictation for a composer (RUN-027; the shared capture the
// agents area reuses). Holds the microphone while the control is held,
// transcribes on release through the saved speech endpoint, and hands the
// text back for the caller to place in its draft. The whole audio
// lifecycle lives here — permission, the recorder, the per-utterance cap,
// stream cleanup — so a composer only decides when to start and stop.

import * as React from "react";
import { transcribeAudio } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";

/** What the control is doing. */
export type DictationStatus = "idle" | "requesting" | "recording" | "transcribing" | "error" | "denied";

/** Longest single utterance kept: well inside every provider's upload cap. */
export const DICTATION_MAX_MS = 60_000;
/** A tap shorter than this holds no useful speech and is dropped. */
export const DICTATION_MIN_MS = 500;
/** How often the elapsed readout updates while recording. */
export const DICTATION_TICK_MS = 200;

/** The browser pieces the hook needs; absent on a host without capture. */
export interface DictationDevices {
  getUserMedia?: ((constraints: MediaStreamConstraints) => Promise<MediaStream>) | undefined;
  createRecorder?: ((stream: MediaStream) => MediaRecorder) | undefined;
}

/** The transcription step, injectable so tests need no server. */
export type DictationTranscriber = (audio: Blob) => Promise<string>;

export interface UseDictationOptions {
  /** Called with recognized text after a recording is transcribed. */
  onResult: (text: string) => void;
  /** Overrides for the browser surface (tests use their own). */
  devices?: DictationDevices | undefined;
  /** Overrides the upload (tests). */
  transcribe?: DictationTranscriber | undefined;
}

export interface DictationHandle {
  status: DictationStatus;
  /** Whole seconds recorded so far; 0 whenever nothing is recording. */
  elapsedSeconds: number;
  /** A message for the error state, when the failure has one. */
  errorMessage: string | null;
  /** True when this browser can capture audio at all. */
  supported: boolean;
  /** Begin capture; a press while busy is ignored. */
  start: () => Promise<void>;
  /** Finish capture and transcribe; a press release calls this. */
  stop: () => void;
  /** Abandon the recording without transcribing. */
  cancel: () => void;
  /** Leave the error or denied state and allow a fresh press. */
  reset: () => void;
}

/** The capture surface as this browser offers it. */
export function browserDictationDevices(): DictationDevices {
  const media = globalThis.navigator?.mediaDevices;
  const getUserMedia = media?.getUserMedia?.bind(media);
  const Recorder = (globalThis as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder;
  return {
    getUserMedia: getUserMedia === undefined ? undefined : (constraints) => getUserMedia(constraints),
    createRecorder: Recorder === undefined ? undefined : (stream) => new Recorder(stream, recorderOptions()),
  };
}

/** First container this browser records that the server can transcribe. */
function recorderOptions(): MediaRecorderOptions {
  const Recorder = (globalThis as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder;
  const candidates = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus"];
  const mimeType =
    Recorder !== undefined && typeof Recorder.isTypeSupported === "function"
      ? candidates.find((type) => Recorder.isTypeSupported(type))
      : undefined;
  return mimeType === undefined ? {} : { mimeType };
}

/** Join dictated text onto a draft without running words together. */
export function appendDictation(draft: string, addition: string): string {
  const text = addition.trim();
  if (text === "") return draft;
  if (draft === "") return text;
  return /\s$/.test(draft) ? `${draft}${text}` : `${draft} ${text}`;
}

function errorMessageOf(error: unknown): string {
  if (error instanceof Error && error.message !== "") return error.message;
  return "Voice input failed";
}

function isPermissionDenial(error: unknown): boolean {
  const name = (error as { name?: unknown } | null)?.name;
  return name === "NotAllowedError" || name === "SecurityError";
}

async function defaultTranscribe(audio: Blob): Promise<string> {
  const answer = await transcribeAudio(getAppClient(), audio);
  return answer.text;
}

/**
 * Capture hook. Holding the control calls `start` and releasing it calls
 * `stop`; a short tap is discarded, a long one is cut at the cap, and an
 * error or a refused microphone stays visible until `reset`.
 */
export function useDictation({ onResult, devices, transcribe }: UseDictationOptions): DictationHandle {
  const [status, setStatus] = React.useState<DictationStatus>("idle");
  const [elapsedMs, setElapsedMs] = React.useState(0);
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  const liveDevices = devices ?? browserDictationDevices();
  const supported = liveDevices.getUserMedia !== undefined && liveDevices.createRecorder !== undefined;

  const onResultRef = React.useRef(onResult);
  onResultRef.current = onResult;
  const transcribeRef = React.useRef<DictationTranscriber>(transcribe ?? defaultTranscribe);
  transcribeRef.current = transcribe ?? defaultTranscribe;
  const devicesRef = React.useRef(liveDevices);
  devicesRef.current = liveDevices;

  const statusRef = React.useRef<DictationStatus>("idle");
  const streamRef = React.useRef<MediaStream | null>(null);
  const recorderRef = React.useRef<MediaRecorder | null>(null);
  const chunksRef = React.useRef<Blob[]>([]);
  const startedAtRef = React.useRef(0);
  const tickRef = React.useRef<ReturnType<typeof setInterval> | null>(null);
  const capRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const cancelledRef = React.useRef(false);
  // Set when the control is released while the microphone prompt is still
  // open: nothing started yet, so the granted stream is handed straight
  // back instead of recording until the cap.
  const releasedRef = React.useRef(false);

  const setPhase = React.useCallback((next: DictationStatus) => {
    statusRef.current = next;
    setStatus(next);
  }, []);

  const releaseStream = React.useCallback(() => {
    for (const track of streamRef.current?.getTracks() ?? []) track.stop();
    streamRef.current = null;
  }, []);

  const clearTimers = React.useCallback(() => {
    if (tickRef.current !== null) clearInterval(tickRef.current);
    if (capRef.current !== null) clearTimeout(capRef.current);
    tickRef.current = null;
    capRef.current = null;
  }, []);

  const finishRecording = React.useCallback(async () => {
    clearTimers();
    const recorder = recorderRef.current;
    recorderRef.current = null;
    const held = Date.now() - startedAtRef.current;
    const blob = new Blob(chunksRef.current, { type: recorder?.mimeType || "audio/webm" });
    chunksRef.current = [];
    releaseStream();
    if (cancelledRef.current || held < DICTATION_MIN_MS || blob.size === 0) {
      cancelledRef.current = false;
      setElapsedMs(0);
      setPhase("idle");
      return;
    }
    setElapsedMs(0);
    setPhase("transcribing");
    try {
      const text = await transcribeRef.current(blob);
      setPhase("idle");
      if (text.trim() !== "") onResultRef.current(text);
    } catch (error) {
      setErrorMessage(errorMessageOf(error));
      setPhase("error");
    }
  }, [clearTimers, releaseStream, setPhase]);

  const stop = React.useCallback(() => {
    const recorder = recorderRef.current;
    if (recorder === null) {
      if (statusRef.current === "requesting") releasedRef.current = true;
      return;
    }
    if (recorder.state !== "inactive") recorder.stop();
    else void finishRecording();
  }, [finishRecording]);

  // Read through a widening helper: the phase changes while the microphone
  // prompt is open, which the compiler cannot see from this closure.
  const stillRequesting = (): boolean => (statusRef.current as string) === "requesting";

  const start = React.useCallback(async (): Promise<void> => {
    if (
      !supported ||
      statusRef.current === "requesting" ||
      statusRef.current === "recording" ||
      statusRef.current === "transcribing"
    ) {
      return;
    }
    cancelledRef.current = false;
    releasedRef.current = false;
    setErrorMessage(null);
    setPhase("requesting");
    let stream: MediaStream;
    try {
      stream = await devicesRef.current.getUserMedia!({ audio: true });
    } catch (error) {
      setErrorMessage(
        isPermissionDenial(error)
          ? "Microphone access is blocked. Allow it in the browser's site settings, then try again."
          : errorMessageOf(error)
      );
      setPhase(isPermissionDenial(error) ? "denied" : "error");
      return;
    }
    // Released before the microphone answered: hand the device straight back.
    if (!stillRequesting() || releasedRef.current) {
      for (const track of stream.getTracks()) track.stop();
      releasedRef.current = false;
      // Nothing was started, so the control returns to rest whether the
      // release or a cancel got here first.
      setPhase("idle");
      return;
    }
    streamRef.current = stream;
    let recorder: MediaRecorder;
    try {
      recorder = devicesRef.current.createRecorder!(stream);
    } catch (error) {
      releaseStream();
      setErrorMessage(errorMessageOf(error));
      setPhase("error");
      return;
    }
    recorderRef.current = recorder;
    chunksRef.current = [];
    recorder.ondataavailable = (event: BlobEvent) => {
      if (event.data.size > 0) chunksRef.current.push(event.data);
    };
    recorder.onstop = () => {
      if (recorderRef.current === recorder) void finishRecording();
    };
    try {
      recorder.start();
    } catch (error) {
      recorderRef.current = null;
      releaseStream();
      setErrorMessage(errorMessageOf(error));
      setPhase("error");
      return;
    }
    startedAtRef.current = Date.now();
    setElapsedMs(0);
    setPhase("recording");
    tickRef.current = setInterval(() => setElapsedMs(Date.now() - startedAtRef.current), DICTATION_TICK_MS);
    capRef.current = setTimeout(stop, DICTATION_MAX_MS);
  }, [finishRecording, releaseStream, setPhase, stop, supported]);

  const cancel = React.useCallback(() => {
    if (statusRef.current !== "requesting" && statusRef.current !== "recording") return;
    cancelledRef.current = true;
    releasedRef.current = false;
    clearTimers();
    const recorder = recorderRef.current;
    if (recorder === null) {
      // Still waiting on the microphone: the answer above hands it back.
      setPhase("idle");
      return;
    }
    recorderRef.current = null;
    if (recorder.state !== "inactive") {
      recorder.onstop = null;
      recorder.stop();
      chunksRef.current = [];
      clearTimers();
      releaseStream();
      setElapsedMs(0);
      setPhase("idle");
      cancelledRef.current = false;
    } else {
      void finishRecording();
    }
  }, [clearTimers, finishRecording, releaseStream, setPhase]);

  const reset = React.useCallback(() => {
    if (statusRef.current !== "error" && statusRef.current !== "denied") return;
    setErrorMessage(null);
    setPhase("idle");
  }, [setPhase]);

  React.useEffect(() => {
    // Unmounting abandons an in-flight utterance: the device is handed
    // back and no text is placed in a draft that no longer exists.
    return () => {
      clearTimers();
      const recorder = recorderRef.current;
      recorderRef.current = null;
      if (recorder !== null && recorder.state !== "inactive") {
        cancelledRef.current = true;
        recorder.stop();
      }
      chunksRef.current = [];
      releaseStream();
    };
  }, [clearTimers, releaseStream]);

  return {
    status,
    elapsedSeconds: Math.floor(elapsedMs / 1000),
    errorMessage,
    supported,
    start,
    stop,
    cancel,
    reset,
  };
}
