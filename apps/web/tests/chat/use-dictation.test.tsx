/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { SWRConfig } from "swr";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { getSTTConfig, transcribeAudio } = vi.hoisted(() => ({
  getSTTConfig: vi.fn(),
  transcribeAudio: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  AssistantService: class {
    getSTTConfig = getSTTConfig;
    transcribeAudio = transcribeAudio;
  },
}));

import { useDictation } from "../../core/hooks/use-dictation";

// --- Minimal MediaRecorder / getUserMedia doubles (happy-dom has neither) ---

const trackStop = vi.fn();

class MockRecorder {
  static isTypeSupported = () => true;
  state: "inactive" | "recording" = "inactive";
  ondataavailable: ((e: { data: Blob }) => void) | null = null;
  onstop: (() => void) | null = null;
  constructor(
    public stream: unknown,
    public options?: unknown
  ) {}
  start() {
    this.state = "recording";
  }
  stop() {
    this.state = "inactive";
    // Real recorders flush a final chunk before firing onstop.
    this.ondataavailable?.({ data: new Blob(["audio"], { type: "audio/webm" }) });
    this.onstop?.();
  }
}

const getUserMedia = vi.fn();

function installMediaSupport() {
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia },
  });
  (globalThis as unknown as { MediaRecorder: unknown }).MediaRecorder = MockRecorder;
}

function makeStream() {
  return { getTracks: () => [{ stop: trackStop }] };
}

// Isolate SWR's cache per render so config from one test doesn't leak.
function wrapper({ children }: { children: ReactNode }) {
  return <SWRConfig value={{ provider: () => new Map(), dedupingInterval: 0 }}>{children}</SWRConfig>;
}

let nowMs = 0;

beforeEach(() => {
  nowMs = 0;
  vi.spyOn(Date, "now").mockImplementation(() => nowMs);
  getUserMedia.mockReset().mockResolvedValue(makeStream());
  transcribeAudio.mockReset().mockResolvedValue({ text: "hello world" });
  getSTTConfig.mockReset().mockResolvedValue({
    base_url: "https://x",
    model_name: "whisper-1",
    has_api_key: true,
    last_verified_at: null,
  });
  trackStop.mockReset();
  installMediaSupport();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useDictation", () => {
  it("routes to not-configured when the endpoint has no key, without recording", async () => {
    getSTTConfig.mockResolvedValue({
      base_url: "",
      model_name: "",
      has_api_key: false,
      last_verified_at: null,
    });
    const { result } = renderHook(() => useDictation({ onResult: vi.fn() }), { wrapper });

    await waitFor(() => expect(result.current.isUnconfigured).toBe(true));

    await act(async () => {
      await result.current.start();
    });

    expect(result.current.status).toBe("unconfigured");
    expect(getUserMedia).not.toHaveBeenCalled();
  });

  it("surfaces permission-denied distinctly from a generic error", async () => {
    getUserMedia.mockRejectedValue(Object.assign(new Error("no"), { name: "NotAllowedError" }));
    const { result } = renderHook(() => useDictation({ onResult: vi.fn() }), { wrapper });
    await waitFor(() => expect(result.current.isUnconfigured).toBe(false));

    await act(async () => {
      await result.current.start();
    });

    expect(result.current.status).toBe("denied");
  });

  it("records, transcribes, and appends the recognized text", async () => {
    const onResult = vi.fn();
    const { result } = renderHook(() => useDictation({ onResult }), { wrapper });
    await waitFor(() => expect(result.current.isUnconfigured).toBe(false));

    await act(async () => {
      await result.current.start();
    });
    expect(result.current.status).toBe("recording");

    nowMs = 1500; // held long enough to count as real speech
    await act(async () => {
      result.current.stop();
    });

    await waitFor(() => expect(onResult).toHaveBeenCalledWith("hello world"));
    expect(transcribeAudio).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(trackStop).toHaveBeenCalled(); // stream released
  });

  // The backend reports transcribe failures as `{ error, detail }` (see
  // apps/api/pi_dash/assistant/views/transcribe.py). These pin that shape.
  it("routes a stt_config_missing response to the unconfigured state", async () => {
    transcribeAudio.mockRejectedValue({ error: "stt_config_missing", detail: "Configure dictation in Settings." });
    const onResult = vi.fn();
    const { result } = renderHook(() => useDictation({ onResult }), { wrapper });
    await waitFor(() => expect(result.current.isUnconfigured).toBe(false));

    await act(async () => {
      await result.current.start();
    });
    nowMs = 1500;
    await act(async () => {
      result.current.stop();
    });

    await waitFor(() => expect(result.current.status).toBe("unconfigured"));
    expect(result.current.isUnconfigured).toBe(true);
    expect(onResult).not.toHaveBeenCalled();
  });

  it("shows the backend detail for a provider failure", async () => {
    transcribeAudio.mockRejectedValue({
      error: "provider_auth_failed",
      detail: "The dictation provider rejected the API key.",
    });
    const { result } = renderHook(() => useDictation({ onResult: vi.fn() }), { wrapper });
    await waitFor(() => expect(result.current.isUnconfigured).toBe(false));

    await act(async () => {
      await result.current.start();
    });
    nowMs = 1500;
    await act(async () => {
      result.current.stop();
    });

    await waitFor(() => expect(result.current.status).toBe("error"));
    expect(result.current.errorMessage).toBe("The dictation provider rejected the API key.");
  });

  it("discards an accidental sub-threshold tap without transcribing", async () => {
    const onResult = vi.fn();
    const { result } = renderHook(() => useDictation({ onResult }), { wrapper });
    await waitFor(() => expect(result.current.isUnconfigured).toBe(false));

    await act(async () => {
      await result.current.start();
    });
    nowMs = 100; // released almost immediately
    await act(async () => {
      result.current.stop();
    });

    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(transcribeAudio).not.toHaveBeenCalled();
    expect(trackStop).toHaveBeenCalled();
  });

  it("releases the microphone stream on unmount", async () => {
    const { result, unmount } = renderHook(() => useDictation({ onResult: vi.fn() }), { wrapper });
    await waitFor(() => expect(result.current.isUnconfigured).toBe(false));
    await act(async () => {
      await result.current.start();
    });
    expect(result.current.status).toBe("recording");

    unmount();
    expect(trackStop).toHaveBeenCalled();
  });
});
