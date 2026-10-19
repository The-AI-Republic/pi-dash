// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Push-to-talk capture (RUN-027): hold to record, release to transcribe,
// a tap is dropped, the cap stops a long hold, a refused microphone reads
// as denial, and every outcome is visible through the control's state.
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  appendDictation,
  browserDictationDevices,
  DICTATION_MAX_MS,
  DICTATION_MIN_MS,
  useDictation,
  type DictationDevices,
} from "./useDictation.js";

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

function fakeStream(): MediaStream {
  const track = { stop: vi.fn() };
  return { getTracks: () => [track] } as unknown as MediaStream;
}

function setup(
  overrides: { transcribe?: (audio: Blob) => Promise<string>; getUserMedia?: () => Promise<MediaStream> } = {}
) {
  const stream = fakeStream();
  const recorders: FakeRecorder[] = [];
  const devices: DictationDevices = {
    getUserMedia: overrides.getUserMedia ?? (async () => stream),
    createRecorder: () => {
      const recorder = new FakeRecorder();
      recorders.push(recorder);
      return recorder as unknown as MediaRecorder;
    },
  };
  const onResult = vi.fn();
  const transcribe = overrides.transcribe ?? (async () => " spoken words ");
  const view = renderHook(() => useDictation({ onResult, devices, transcribe }));
  return { view, onResult, recorders, stream, transcribe };
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("appendDictation", () => {
  it("joins text without running words together", () => {
    expect(appendDictation("", " hello ")).toBe("hello");
    expect(appendDictation("  ", "hello")).toBe("  hello");
    expect(appendDictation("draft", "more")).toBe("draft more");
    expect(appendDictation("draft ", "more")).toBe("draft more");
    expect(appendDictation("draft", "   ")).toBe("draft");
  });
});

describe("useDictation", () => {
  it("reports support from the browser surface", () => {
    const { view } = setup();
    expect(view.result.current.supported).toBe(true);
    const bare = renderHook(() => useDictation({ onResult: vi.fn(), devices: {} }));
    expect(bare.result.current.supported).toBe(false);
  });

  it("records while held and appends the transcription on release", async () => {
    const { view, onResult, recorders, stream } = setup();
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("recording");
    expect(recorders[0]!.state).toBe("recording");
    act(() => vi.advanceTimersByTime(1_500));
    expect(view.result.current.elapsedSeconds).toBe(1);
    act(() => view.result.current.stop());
    await waitFor(() => expect(onResult).toHaveBeenCalledWith(" spoken words "));
    expect(view.result.current.status).toBe("idle");
    expect((stream.getTracks()[0] as unknown as { stop: ReturnType<typeof vi.fn> }).stop).toHaveBeenCalled();
  });

  it("transcribes a recorder that names no container", async () => {
    const stream = fakeStream();
    const recorder = new FakeRecorder();
    recorder.mimeType = "";
    const onResult = vi.fn();
    const view = renderHook(() =>
      useDictation({
        onResult,
        devices: {
          getUserMedia: async () => stream,
          createRecorder: () => recorder as unknown as MediaRecorder,
        },
        transcribe: async () => "container free",
      })
    );
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.stop());
    await waitFor(() => expect(onResult).toHaveBeenCalledWith("container free"));
  });

  it("drops a tap shorter than the minimum", async () => {
    const transcribe = vi.fn(async () => "unused");
    const { view, onResult } = setup({ transcribe });
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS - 100));
    act(() => view.result.current.stop());
    await waitFor(() => expect(view.result.current.status).toBe("idle"));
    expect(onResult).not.toHaveBeenCalled();
    expect(transcribe).not.toHaveBeenCalled();
  });

  it("stops a long hold at the cap and transcribes", async () => {
    const { view, onResult } = setup();
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MAX_MS + 100));
    await waitFor(() => expect(onResult).toHaveBeenCalled());
  });

  it("ignores a start while one is running and while transcribing", async () => {
    let finish: (text: string) => void = () => undefined;
    const transcribe = vi.fn<(audio: Blob) => Promise<string>>(() => new Promise<string>((done) => (finish = done)));
    const { view } = setup({ transcribe });
    await act(async () => view.result.current.start());
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.stop());
    await waitFor(() => expect(view.result.current.status).toBe("transcribing"));
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("transcribing");
    expect(transcribe).toHaveBeenCalledTimes(1);
    await act(async () => finish("done"));
  });

  it("reads a refused microphone as denial with a hint, and reset clears it", async () => {
    const denied = Object.assign(new Error("Permission denied"), { name: "NotAllowedError" });
    const { view } = setup({ getUserMedia: async () => Promise.reject(denied) });
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("denied");
    expect(view.result.current.errorMessage).toContain("blocked");
    act(() => view.result.current.reset());
    expect(view.result.current.status).toBe("idle");
    expect(view.result.current.errorMessage).toBeNull();
    // Resetting once more, now from idle, changes nothing.
    act(() => view.result.current.reset());
    expect(view.result.current.status).toBe("idle");
    expect(view.result.current.errorMessage).toBeNull();
  });

  it("treats a security refusal as a denied microphone", async () => {
    const refused = Object.assign(new Error("blocked by policy"), { name: "SecurityError" });
    const { view } = setup({ getUserMedia: async () => Promise.reject(refused) });
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("denied");
  });

  it("reads any other capture failure as an error", async () => {
    const { view } = setup({ getUserMedia: async () => Promise.reject(new Error("no device")) });
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("error");
    expect(view.result.current.errorMessage).toBe("no device");
    act(() => view.result.current.reset());
    expect(view.result.current.status).toBe("idle");
  });

  it("reports a failed transcription and lets a fresh press retry", async () => {
    const transcribe = vi
      .fn<(audio: Blob) => Promise<string>>()
      .mockRejectedValueOnce(new Error("provider down"))
      .mockResolvedValueOnce("second try");
    const { view, onResult } = setup({ transcribe });
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.stop());
    await waitFor(() => expect(view.result.current.status).toBe("error"));
    expect(view.result.current.errorMessage).toBe("provider down");
    act(() => view.result.current.reset());
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.stop());
    await waitFor(() => expect(onResult).toHaveBeenCalledWith("second try"));
  });

  it("survives a failure with no message", async () => {
    const { view } = setup({ transcribe: async () => Promise.reject(new Error("")) });
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.stop());
    await waitFor(() => expect(view.result.current.errorMessage).toBe("Voice input failed"));
  });

  it("keeps an empty transcription out of the draft", async () => {
    const { view, onResult } = setup({ transcribe: async () => "   " });
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.stop());
    await waitFor(() => expect(view.result.current.status).toBe("idle"));
    expect(onResult).not.toHaveBeenCalled();
  });

  it("cancels a held recording without transcribing", async () => {
    const { view, onResult, recorders, stream } = setup();
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => view.result.current.cancel());
    await waitFor(() => expect(view.result.current.status).toBe("idle"));
    expect(recorders[0]!.onstop).toBeNull();
    expect(onResult).not.toHaveBeenCalled();
    expect((stream.getTracks()[0] as unknown as { stop: ReturnType<typeof vi.fn> }).stop).toHaveBeenCalled();
  });

  it("cancels while the microphone is still being granted", async () => {
    let grant: (stream: MediaStream) => void = () => undefined;
    const stream = fakeStream();
    const { view, recorders } = setup({
      getUserMedia: () => new Promise<MediaStream>((done) => (grant = done)),
    });
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = view.result.current.start();
    });
    expect(view.result.current.status).toBe("requesting");
    act(() => view.result.current.cancel());
    expect(view.result.current.status).toBe("idle");
    await act(async () => {
      grant(stream);
      await pending;
    });
    expect(recorders).toHaveLength(0);
    expect((stream.getTracks()[0] as unknown as { stop: ReturnType<typeof vi.fn> }).stop).toHaveBeenCalled();
  });

  it("does not leave the microphone recording when released before it was granted", async () => {
    let grant: (stream: MediaStream) => void = () => undefined;
    const stream = fakeStream();
    const { view, recorders } = setup({
      getUserMedia: () => new Promise<MediaStream>((done) => (grant = done)),
    });
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = view.result.current.start();
    });
    // A quick press and release: the pointer went up while the browser was
    // still asking for permission.
    act(() => view.result.current.stop());
    await act(async () => {
      grant(stream);
      await pending;
    });
    expect(recorders).toHaveLength(0);
    expect(view.result.current.status).toBe("idle");
    expect((stream.getTracks()[0] as unknown as { stop: ReturnType<typeof vi.fn> }).stop).toHaveBeenCalled();
  });

  it("reports a recorder that cannot be opened", async () => {
    const view = renderHook(() =>
      useDictation({
        onResult: vi.fn(),
        devices: {
          getUserMedia: async () => fakeStream(),
          createRecorder: () => {
            throw new Error("recorder unavailable");
          },
        },
      })
    );
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("error");
    expect(view.result.current.errorMessage).toBe("recorder unavailable");
  });

  it("reports a recorder that fails to start", async () => {
    const view = renderHook(() =>
      useDictation({
        onResult: vi.fn(),
        devices: {
          getUserMedia: async () => fakeStream(),
          createRecorder: () =>
            ({
              state: "inactive",
              mimeType: "audio/webm",
              start: () => {
                throw new Error("device busy");
              },
            }) as unknown as MediaRecorder,
        },
      })
    );
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("error");
    expect(view.result.current.errorMessage).toBe("device busy");
  });

  it("finishes a capture whose recorder already went idle", async () => {
    // The recorder ended by itself without its stop event reaching the
    // hook, so the release finishes the capture directly.
    const { view, onResult, recorders } = setup();
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    recorders[0]!.ondataavailable?.({ data: new Blob(["audio-bytes"], { type: "audio/webm" }) });
    recorders[0]!.state = "inactive";
    act(() => view.result.current.stop());
    await waitFor(() => expect(onResult).toHaveBeenCalled());
  });

  it("names a failure with no message at all", async () => {
    const { view } = setup({
      getUserMedia: async () => {
        throw null;
      },
    });
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("error");
    expect(view.result.current.errorMessage).toBe("Voice input failed");
  });

  it("finishes a recording whose recorder already went idle", async () => {
    // A release that arrives after the recorder ended by itself finds an
    // idle recorder and finishes the capture directly.
    const { view, onResult } = setup();
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    act(() => {
      view.result.current.stop();
      view.result.current.stop();
    });
    await waitFor(() => expect(onResult).toHaveBeenCalledTimes(1));
  });

  it("cancels a recorder that had already stopped", async () => {
    const { view, recorders, onResult } = setup();
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    // The recorder ended by itself (its own stop event fired), then the
    // person cancelled: the capture still resolves instead of hanging.
    recorders[0]!.stop();
    act(() => view.result.current.cancel());
    await waitFor(() => expect(onResult).toHaveBeenCalledTimes(1));
  });

  it("cancels a capture whose recorder went idle without a stop event", async () => {
    // The recorder ended on its own but its stop event never reached the
    // hook (a dropped callback), so the cancel has to finish the capture.
    const { view, recorders } = setup();
    await act(async () => view.result.current.start());
    recorders[0]!.ondataavailable?.({ data: new Blob(["audio-bytes"], { type: "audio/webm" }) });
    recorders[0]!.onstop = null;
    recorders[0]!.state = "inactive";
    act(() => view.result.current.cancel());
    await waitFor(() => expect(view.result.current.status).toBe("idle"));
  });

  it("drops a recording with no audio", async () => {
    const { view, onResult, recorders } = setup();
    await act(async () => view.result.current.start());
    act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
    recorders[0]!.ondataavailable = null;
    act(() => recorders[0]!.onstop?.());
    await waitFor(() => expect(view.result.current.status).toBe("idle"));
    expect(onResult).not.toHaveBeenCalled();
  });

  it("stops the device and the timers on unmount", async () => {
    const { view, stream, onResult } = setup();
    await act(async () => view.result.current.start());
    view.unmount();
    expect((stream.getTracks()[0] as unknown as { stop: ReturnType<typeof vi.fn> }).stop).toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(DICTATION_MAX_MS * 2));
    expect(onResult).not.toHaveBeenCalled();
  });

  it("ignores a release and a cancel while idle", () => {
    const { view } = setup();
    act(() => view.result.current.stop());
    act(() => view.result.current.cancel());
    expect(view.result.current.status).toBe("idle");
  });

  it("does nothing for a press when capture is unsupported", async () => {
    const view = renderHook(() => useDictation({ onResult: vi.fn(), devices: {} }));
    await act(async () => view.result.current.start());
    expect(view.result.current.status).toBe("idle");
  });

  it("uses the browser surface when no devices are injected", async () => {
    const getUserMedia = vi.fn(async () => fakeStream());
    class BrowserRecorder extends FakeRecorder {}
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
    vi.stubGlobal("MediaRecorder", BrowserRecorder);
    const clientModule = await import("../../core/api/client.js");
    clientModule.createAppClient("https://api.test", {} as never);
    try {
      const onResult = vi.fn();
      const view = renderHook(() => useDictation({ onResult }));
      expect(view.result.current.supported).toBe(true);
      await act(async () => view.result.current.start());
      expect(getUserMedia).toHaveBeenCalledWith({ audio: true });
      act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
      act(() => view.result.current.stop());
      await waitFor(() => expect(view.result.current.status).toBe("transcribing"));
    } finally {
      clientModule.resetAppClient();
      vi.unstubAllGlobals();
    }
  });

  it("transcribes through the speech endpoint by default", async () => {
    // With no transcriber injected, the utterance goes to the shared
    // speech endpoint through the app client and its answer lands in the
    // draft.
    const seen: string[] = [];
    const stubFetch = vi.fn(async (input: RequestInfo | URL) => {
      seen.push(String(input));
      return new Response(JSON.stringify({ text: "typed by speech" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });
    vi.stubGlobal("fetch", stubFetch);
    const clientModule = await import("../../core/api/client.js");
    clientModule.createAppClient("https://api.test");
    const onResult = vi.fn();
    const view = renderHook(() =>
      useDictation({
        onResult,
        devices: {
          getUserMedia: async () => fakeStream(),
          createRecorder: () => new FakeRecorder() as unknown as MediaRecorder,
        },
      })
    );
    try {
      await act(async () => view.result.current.start());
      act(() => vi.advanceTimersByTime(DICTATION_MIN_MS + 10));
      act(() => view.result.current.stop());
      await waitFor(() => expect(onResult).toHaveBeenCalledWith("typed by speech"));
      expect(seen).toEqual([
        "https://api.test/auth/get-csrf-token/",
        "https://api.test/api/users/me/ai-assistant/transcribe/",
      ]);
    } finally {
      clientModule.resetAppClient();
      vi.unstubAllGlobals();
    }
  });
});

describe("browserDictationDevices", () => {
  it("reports no capture on a bare host", () => {
    const devices = browserDictationDevices();
    expect(devices.getUserMedia).toBeUndefined();
    expect(devices.createRecorder).toBeUndefined();
  });

  it("offers the browser's own microphone and a container it supports", async () => {
    const stream = fakeStream();
    const getUserMedia = vi.fn(async () => stream);
    const created: unknown[] = [];
    class BrowserRecorder {
      static isTypeSupported(type: string): boolean {
        return type === "audio/webm";
      }
      constructor(input: unknown, options: unknown) {
        created.push({ input, options });
      }
    }
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
    vi.stubGlobal("MediaRecorder", BrowserRecorder);
    try {
      const devices = browserDictationDevices();
      expect(devices.getUserMedia).toBeDefined();
      expect(devices.createRecorder).toBeDefined();
      expect(await devices.getUserMedia!({ audio: true })).toBe(stream);
      const recorder = devices.createRecorder!(stream);
      expect(recorder).toBeInstanceOf(BrowserRecorder);
      expect(created).toEqual([{ input: stream, options: { mimeType: "audio/webm" } }]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("falls back to the browser's default container when none is advertised", () => {
    const created: unknown[] = [];
    class PlainRecorder {
      constructor(_input: unknown, options: unknown) {
        created.push(options);
      }
    }
    vi.stubGlobal("MediaRecorder", PlainRecorder);
    try {
      browserDictationDevices().createRecorder!(fakeStream());
      expect(created).toEqual([{}]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("reads a browser without a container preference", () => {
    const created: unknown[] = [];
    class NoPreference {
      static isTypeSupported(): boolean {
        return false;
      }
      constructor(_input: unknown, options: unknown) {
        created.push(options);
      }
    }
    vi.stubGlobal("MediaRecorder", NoPreference);
    try {
      browserDictationDevices().createRecorder!(fakeStream());
      expect(created).toEqual([{}]);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
