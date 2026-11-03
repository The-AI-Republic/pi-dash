// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer dictation wiring (AGT-050/064): the speech read flows to the
// cell, uploads land in the draft, and a kill-switch refusal re-reads
// the config so the microphone hides.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DICTATION_MIN_MS, sttConfigKeys } from "../../../shared/dictation/index.js";
import { useComposerDictation } from "./useComposerDictation.js";

class FakeRecorder {
  state: RecordingState = "inactive";
  mimeType = "audio/webm";
  ondataavailable: ((event: { data: Blob }) => void) | null = null;
  onstop: (() => void) | null = null;

  start(): void {
    this.state = "recording";
  }

  stop(): void {
    this.state = "inactive";
    this.ondataavailable?.({ data: new Blob(["audio-bytes"], { type: this.mimeType }) });
    this.onstop?.();
  }
}

const STT_KEYED = {
  base_url: "https://stt.test",
  model_name: "whisper",
  has_api_key: true,
  last_verified_at: null,
  enabled: true,
};

function setup(transcribe: (url: string) => Response) {
  const seen: string[] = [];
  const stubFetch = vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    seen.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return new Response(JSON.stringify({ csrf_token: "csrf" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }
    if (url.endsWith("/api/users/me/ai-assistant/stt-config/")) {
      return new Response(JSON.stringify(STT_KEYED), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }
    return transcribe(url);
  });
  vi.stubGlobal("fetch", stubFetch);
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: async () => fakeStream() } });
  vi.stubGlobal("MediaRecorder", FakeRecorder);
  createAppClient("https://api.test");
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onResult = vi.fn();
  const view = renderHook(() => useComposerDictation(onResult), {
    wrapper: ({ children }: { children?: unknown }) => (
      <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
    ),
  });
  return { view, onResult, queryClient, seen };
}

function fakeStream(): MediaStream {
  const track = { stop: vi.fn() };
  return { getTracks: () => [track] } as unknown as MediaStream;
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("useComposerDictation", () => {
  it("hands the speech read and a working capture handle to the cell", async () => {
    const { view, onResult } = setup(
      () =>
        new Response(JSON.stringify({ text: "hello there" }), {
          status: 200,
          headers: { "content-type": "application/json" },
        })
    );
    await waitFor(() => expect(view.result.current.speech).toEqual(STT_KEYED));
    expect(view.result.current.handle.supported).toBe(true);
    await act(async () => view.result.current.handle.start());
    expect(view.result.current.handle.status).toBe("recording");
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.handle.stop());
    await waitFor(() => expect(onResult).toHaveBeenCalledWith("hello there"));
    expect(view.result.current.handle.status).toBe("idle");
  });

  it("re-reads the speech config when the switch refuses the upload", async () => {
    const { view, seen } = setup(
      () =>
        new Response(JSON.stringify({ error: "dictation_disabled", detail: "voice dictation is off" }), {
          status: 404,
          headers: { "content-type": "application/json" },
        })
    );
    await waitFor(() => expect(view.result.current.speech).toEqual(STT_KEYED));
    const readsBefore = seen.filter((url) => url.endsWith("/api/users/me/ai-assistant/stt-config/")).length;
    await act(async () => view.result.current.handle.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.handle.stop());
    await waitFor(() => expect(view.result.current.handle.status).toBe("error"));
    await waitFor(() =>
      expect(seen.filter((url) => url.endsWith("/api/users/me/ai-assistant/stt-config/")).length).toBeGreaterThan(
        readsBefore
      )
    );
  });

  it("leaves the config alone when the upload fails for any other reason", async () => {
    const { view, queryClient, seen } = setup(
      () =>
        new Response(JSON.stringify({ error: "transcribe_failed", detail: "provider down" }), {
          status: 500,
          headers: { "content-type": "application/json" },
        })
    );
    await waitFor(() => expect(view.result.current.speech).toEqual(STT_KEYED));
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await act(async () => view.result.current.handle.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.handle.stop());
    await waitFor(() => expect(view.result.current.handle.status).toBe("error"));
    expect(view.result.current.handle.errorMessage).toBe("provider down");
    expect(invalidate).not.toHaveBeenCalled();
    expect(seen.filter((url) => url.endsWith("/api/users/me/ai-assistant/stt-config/")).length).toBe(1);
  });

  it("reads no speech while the config loads", () => {
    const { view } = setup(
      () =>
        new Response(JSON.stringify({ text: "unused" }), {
          status: 200,
          headers: { "content-type": "application/json" },
        })
    );
    expect(view.result.current.speech).toBeUndefined();
    expect(view.result.current.handle.supported).toBe(true);
  });

  it("reuses the shared speech cache key", () => {
    expect(sttConfigKeys.all()).toEqual(["assistant", "stt-config"]);
  });
});
