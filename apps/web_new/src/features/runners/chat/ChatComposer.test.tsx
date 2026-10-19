// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer (NEWFRONT-549 RUN-032, messaging NEWFRONT-551 RUN-027/030): the
// gate reason renders, the draft is controlled, Enter sends without a
// newline while Shift+Enter keeps it, an empty or trimmed draft cannot
// send, a send in flight locks the box, an in-flight turn swaps the send
// control for stop, and the microphone appears only where dictation is
// offered and the browser can record.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { COMPOSER_OFFLINE } from "./chatState.js";
import {
  CHAT_COMPOSER_PLACEHOLDER,
  CHAT_MIC_LABEL,
  CHAT_SEND_LABEL,
  CHAT_STOP_LABEL,
  ChatComposer,
  formatDictationElapsed,
} from "./ChatComposer.js";

const dictation = vi.hoisted(() => ({ useDictation: vi.fn() }));
vi.mock("../../../shared/dictation/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/dictation/index.js")>();
  return { ...original, useDictation: dictation.useDictation };
});

const idleDictation = {
  status: "idle" as const,
  supported: false,
  elapsedSeconds: 0,
  start: vi.fn(),
  stop: vi.fn(),
  cancel: vi.fn(),
  reset: vi.fn(),
};

beforeEach(() => {
  dictation.useDictation.mockReset();
  dictation.useDictation.mockReturnValue({ ...idleDictation });
});

type Props = Parameters<typeof ChatComposer>[0];

function setup(overrides: Partial<Props> = {}) {
  const props: Props = {
    blockedReason: null,
    draft: "hello",
    onDraftChange: vi.fn(),
    onSend: vi.fn(),
    turnInFlight: false,
    onStop: vi.fn(),
    ...overrides,
  };
  const view = render(<ChatComposer {...props} />);
  return { props, view };
}

describe("formatDictationElapsed", () => {
  it("reads whole seconds as minutes and padded seconds", () => {
    expect(formatDictationElapsed(0)).toBe("0:00");
    expect(formatDictationElapsed(59)).toBe("0:59");
    expect(formatDictationElapsed(61)).toBe("1:01");
  });
});

describe("ChatComposer", () => {
  it("shows the draft, enables send for non-blank text and sends it", async () => {
    const { props } = setup();
    expect(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER)).toHaveValue("hello");
    const send = screen.getByRole("button", { name: CHAT_SEND_LABEL });
    expect(send).toBeEnabled();
    await userEvent.click(send);
    expect(props.onSend).toHaveBeenCalledWith("hello");
    expect(screen.queryByTestId("runner-chat-block-reason")).toBeNull();
  });

  it("sends on Enter without a newline", async () => {
    const { props } = setup();
    const box = screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER);
    await userEvent.type(box, "{Enter}");
    expect(props.onSend).toHaveBeenCalledWith("hello");
  });

  it("leaves Shift+Enter, modifier chords and repeats to the box", async () => {
    const { props } = setup({ draft: "" });
    const box = screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER);
    await userEvent.type(box, "a{Shift>}{Enter}{/Shift}");
    expect(props.onSend).not.toHaveBeenCalled();
    expect(props.onDraftChange).toHaveBeenLastCalledWith("\n");
    vi.mocked(props.onSend).mockClear();
    await userEvent.type(box, "{Control>}{Enter}{/Control}");
    expect(props.onSend).not.toHaveBeenCalled();
    expect(props.onSend).not.toHaveBeenCalledWith(expect.stringContaining("\n"));
  });

  it("keeps send shut for a blank draft and for whitespace alone", async () => {
    const { props } = setup({ draft: "   " });
    const send = screen.getByRole("button", { name: CHAT_SEND_LABEL });
    expect(send).toBeDisabled();
    await userEvent.type(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER), "{Enter}");
    expect(props.onSend).not.toHaveBeenCalled();
  });

  it("shows the blocking reason and disables the box and send", () => {
    setup({ blockedReason: COMPOSER_OFFLINE });
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent(COMPOSER_OFFLINE);
    expect(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER)).toBeDisabled();
    expect(screen.getByRole("button", { name: CHAT_SEND_LABEL })).toBeDisabled();
  });

  it("locks the box while a send is in flight", async () => {
    const { props } = setup({ sending: true });
    expect(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER)).toBeDisabled();
    const send = screen.getByRole("button", { name: CHAT_SEND_LABEL });
    expect(send).toBeDisabled();
    await userEvent.type(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER), "{Enter}");
    expect(props.onSend).not.toHaveBeenCalled();
  });

  it("swaps send for stop while a reply is in flight", () => {
    const { props } = setup({ turnInFlight: true });
    expect(screen.queryByRole("button", { name: CHAT_SEND_LABEL })).toBeNull();
    screen.getByRole("button", { name: CHAT_STOP_LABEL }).click();
    expect(props.onStop).toHaveBeenCalledTimes(1);
  });

  it("offers the microphone only when dictation is available and supported", () => {
    const { view } = setup({ dictationAvailable: true });
    expect(screen.queryByRole("button", { name: CHAT_MIC_LABEL })).toBeNull();

    dictation.useDictation.mockReturnValue({ status: "idle", supported: true, elapsedSeconds: 0 });
    view.rerender(
      <ChatComposer
        blockedReason={null}
        draft="hello"
        onDraftChange={vi.fn()}
        onSend={vi.fn()}
        turnInFlight={false}
        onStop={vi.fn()}
        dictationAvailable
      />
    );
    expect(screen.getByRole("button", { name: CHAT_MIC_LABEL })).toBeInTheDocument();
  });

  it("keeps the microphone hidden when dictation is not offered", () => {
    dictation.useDictation.mockReturnValue({ status: "idle", supported: true, elapsedSeconds: 0 });
    setup({ dictationAvailable: false });
    expect(screen.queryByRole("button", { name: CHAT_MIC_LABEL })).toBeNull();
  });

  it("names a denial and an unreadable dictation failure", () => {
    dictation.useDictation.mockReturnValue({ status: "denied", supported: true, elapsedSeconds: 0 });
    const { view } = setup({ dictationAvailable: true });
    expect(screen.getByTestId("runner-chat-dictation-hint")).toHaveTextContent("Microphone access is blocked");

    dictation.useDictation.mockReturnValue({
      status: "error",
      supported: true,
      elapsedSeconds: 0,
      errorMessage: "Speech service unreachable",
    });
    view.rerender(
      <ChatComposer
        blockedReason={null}
        draft="hello"
        onDraftChange={vi.fn()}
        onSend={vi.fn()}
        turnInFlight={false}
        onStop={vi.fn()}
        dictationAvailable
      />
    );
    expect(screen.getByTestId("runner-chat-dictation-hint")).toHaveTextContent("Speech service unreachable");
  });

  it("falls back to a generic dictation failure line", () => {
    dictation.useDictation.mockReturnValue({ status: "error", supported: true, elapsedSeconds: 0 });
    setup({ dictationAvailable: true });
    expect(screen.getByTestId("runner-chat-dictation-hint")).toHaveTextContent("Voice input failed");
  });

  it("shows the recording clock and the transcribing line", () => {
    dictation.useDictation.mockReturnValue({ status: "recording", supported: true, elapsedSeconds: 65 });
    const { view } = setup({ dictationAvailable: true });
    expect(screen.getByTestId("runner-chat-dictation-hint")).toHaveTextContent("1:05");
    expect(screen.getByRole("button", { name: "Release to transcribe" })).toHaveAttribute("aria-pressed", "true");

    dictation.useDictation.mockReturnValue({ status: "transcribing", supported: true, elapsedSeconds: 65 });
    view.rerender(
      <ChatComposer
        blockedReason={null}
        draft="hello"
        onDraftChange={vi.fn()}
        onSend={vi.fn()}
        turnInFlight={false}
        onStop={vi.fn()}
        dictationAvailable
      />
    );
    expect(screen.getByTestId("runner-chat-dictation-hint")).toHaveTextContent("Transcribing");
  });

  it("holds to dictate and releases to stop", () => {
    const handle = {
      status: "idle",
      supported: true,
      elapsedSeconds: 0,
      start: vi.fn(),
      stop: vi.fn(),
      cancel: vi.fn(),
      reset: vi.fn(),
    };
    dictation.useDictation.mockReturnValue(handle);
    setup({ dictationAvailable: true });
    const mic = screen.getByRole("button", { name: CHAT_MIC_LABEL });
    fireEvent.pointerDown(mic);
    expect(handle.start).toHaveBeenCalledTimes(1);
    fireEvent.pointerUp(mic);
    expect(handle.stop).toHaveBeenCalledTimes(1);
    fireEvent.pointerCancel(mic);
    expect(handle.cancel).toHaveBeenCalledTimes(1);
  });

  it("supports a keyboard hold on the microphone", () => {
    const handle = {
      status: "idle",
      supported: true,
      elapsedSeconds: 0,
      start: vi.fn(),
      stop: vi.fn(),
      cancel: vi.fn(),
      reset: vi.fn(),
    };
    dictation.useDictation.mockReturnValue(handle);
    setup({ dictationAvailable: true });
    const mic = screen.getByRole("button", { name: CHAT_MIC_LABEL });
    fireEvent.keyDown(mic, { key: " " });
    expect(handle.start).toHaveBeenCalledTimes(1);
    fireEvent.keyUp(mic, { key: " " });
    expect(handle.stop).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(mic, { key: "a" });
    expect(handle.start).toHaveBeenCalledTimes(1);
  });

  it("stops dictation when the pointer leaves while recording", () => {
    const handle = {
      status: "recording",
      supported: true,
      elapsedSeconds: 2,
      start: vi.fn(),
      stop: vi.fn(),
      cancel: vi.fn(),
      reset: vi.fn(),
    };
    dictation.useDictation.mockReturnValue(handle);
    setup({ dictationAvailable: true });
    fireEvent.pointerLeave(screen.getByRole("button", { name: "Release to transcribe" }));
    expect(handle.stop).toHaveBeenCalledTimes(1);
  });

  it("leaves an unrelated key release alone", () => {
    const handle = {
      status: "idle",
      supported: true,
      elapsedSeconds: 0,
      start: vi.fn(),
      stop: vi.fn(),
      cancel: vi.fn(),
      reset: vi.fn(),
    };
    dictation.useDictation.mockReturnValue(handle);
    setup({ dictationAvailable: true });
    fireEvent.keyUp(screen.getByRole("button", { name: CHAT_MIC_LABEL }), { key: "Tab" });
    expect(handle.stop).not.toHaveBeenCalled();
  });

  it("does not start on a repeated key down", () => {
    const handle = {
      status: "idle",
      supported: true,
      elapsedSeconds: 0,
      start: vi.fn(),
      stop: vi.fn(),
      cancel: vi.fn(),
      reset: vi.fn(),
    };
    dictation.useDictation.mockReturnValue(handle);
    setup({ dictationAvailable: true });
    fireEvent.keyDown(screen.getByRole("button", { name: CHAT_MIC_LABEL }), { key: "Enter", repeat: true });
    expect(handle.start).not.toHaveBeenCalled();
  });

  it("does not stop on leave when the hold never started recording", () => {
    const handle = {
      status: "requesting",
      supported: true,
      elapsedSeconds: 0,
      start: vi.fn(),
      stop: vi.fn(),
      cancel: vi.fn(),
      reset: vi.fn(),
    };
    dictation.useDictation.mockReturnValue(handle);
    setup({ dictationAvailable: true });
    fireEvent.pointerLeave(screen.getByRole("button", { name: "Release to transcribe" }));
    expect(handle.stop).not.toHaveBeenCalled();
  });

  it("appends a transcript to the draft with dictation spacing", () => {
    const onDraftChange = vi.fn();
    dictation.useDictation.mockReturnValue({ status: "idle", supported: true, elapsedSeconds: 0 });
    setup({ dictationAvailable: true, draft: "draft", onDraftChange });
    const { onResult } = (dictation.useDictation.mock.calls.at(-1)?.[0] ?? {}) as {
      onResult?: (text: string) => void;
    };
    expect(onResult).toBeTypeOf("function");
    onResult?.("spoken words");
    expect(onDraftChange).toHaveBeenCalledWith("draft spoken words");
  });
});
