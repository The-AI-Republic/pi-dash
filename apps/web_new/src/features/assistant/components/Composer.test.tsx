// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer shell (AGT-038/AGT-040/AGT-049): unmodified Enter submits,
// Shift+Enter keeps the newline, blanks never submit, a start locks the
// box, and a missing key locks everything with the reminder line.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { DictationHandle } from "../../../shared/dictation/index.js";
import type { ComposerDictation } from "../lib/dictation.js";
import { Composer } from "./Composer.js";

function renderComposer(props: Partial<React.ComponentProps<typeof Composer>> = {}) {
  const onSubmit = vi.fn();
  const onDraftChange = vi.fn();
  render(<Composer draft="" onDraftChange={onDraftChange} onSubmit={onSubmit} {...props} />);
  return { onSubmit, onDraftChange };
}

describe("Composer", () => {
  it("submits the trimmed draft with unmodified Enter", () => {
    const { onSubmit } = renderComposer({ draft: "  hello  " });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message the assistant" }), { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("hello");
  });

  it("keeps the newline on Shift+Enter and never submits blanks", () => {
    const { onSubmit } = renderComposer({ draft: "line" });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
    const { onSubmit: blankSubmit } = renderComposer({ draft: "   " });
    const blankBox = screen.getAllByRole("textbox", { name: "Message the assistant" })[1] as HTMLElement;
    fireEvent.keyDown(blankBox, { key: "Enter" });
    expect(blankSubmit).not.toHaveBeenCalled();
    fireEvent.click(screen.getAllByRole("button", { name: "Send message" })[1] as HTMLElement);
    expect(blankSubmit).not.toHaveBeenCalled();
  });

  it("submits on Shift+Enter's newline but never on a modifier chord", () => {
    const { onSubmit } = renderComposer({ draft: "chord" });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(box, { key: "Enter", metaKey: true });
    fireEvent.keyDown(box, { key: "Enter", altKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("submits from the send button", () => {
    const { onSubmit } = renderComposer({ draft: "ship it" });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(onSubmit).toHaveBeenCalledWith("ship it");
  });

  it("locks while a start is in flight", () => {
    const { onSubmit } = renderComposer({ draft: "hello", starting: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("locks without a provider key and explains why", () => {
    const { onSubmit } = renderComposer({ draft: "hello", configured: false });
    expect(screen.getByTestId("assistant-composer")).toHaveAttribute("data-locked", "true");
    expect(screen.getByTestId("assistant-composer-reason")).toHaveTextContent(
      "Please set your API key in Settings first to start using AI Assistant."
    );
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.submit(box.closest("form") as HTMLFormElement);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("swaps send for stop while a turn runs, and a plain Enter still posts", () => {
    // The old surface keeps the box editable and posts during a turn; the
    // server owns the race and answers `turn_active`. Swallowing Enter here
    // stranded the draft with no way to tell why (AGT-049).
    const onStop = vi.fn();
    const { onSubmit } = renderComposer({ draft: "hello", running: true, onStop });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Send message" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop response" }));
    expect(onStop).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledWith("hello");
  });

  it("keeps modified Enter and Shift+Enter out of the send path while a turn runs", () => {
    const { onSubmit } = renderComposer({ draft: "hello", running: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("never sends a blank draft while a turn runs", () => {
    const { onSubmit } = renderComposer({ draft: "   ", running: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.keyDown(box, { key: "Enter" });
    fireEvent.submit(box.closest("form") as HTMLFormElement);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("locks the box and shows the posting state while a send is in flight", () => {
    const { onSubmit } = renderComposer({ draft: "hello", posting: true });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    expect(box).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("applies the bordered variant only when docked", () => {
    render(<Composer draft="" onDraftChange={() => {}} onSubmit={() => {}} bordered={false} className="probe" />);
    const form = screen.getByTestId("assistant-composer");
    expect(form).toHaveClass("probe");
    expect(form).not.toHaveClass("border-t");
  });

  it("reports draft edits", () => {
    const { onDraftChange } = renderComposer();
    fireEvent.change(screen.getByRole("textbox", { name: "Message the assistant" }), { target: { value: "typed" } });
    expect(onDraftChange).toHaveBeenCalledWith("typed");
  });
});

function fakeHandle(overrides: Partial<DictationHandle> = {}): DictationHandle {
  return {
    status: "idle",
    elapsedSeconds: 0,
    errorMessage: null,
    captureFailure: null,
    supported: true,
    start: vi.fn(async () => undefined),
    stop: vi.fn(),
    cancel: vi.fn(),
    reset: vi.fn(),
    ...overrides,
  };
}

function keyedDictation(handle: DictationHandle): ComposerDictation {
  return { speech: { enabled: true, has_api_key: true }, handle };
}

describe("Composer dictation", () => {
  it("renders no microphone without the dictation cell", () => {
    renderComposer();
    expect(screen.queryByTestId("assistant-mic")).not.toBeInTheDocument();
    expect(screen.queryByTestId("assistant-dictation-hint")).not.toBeInTheDocument();
  });

  it("hides the microphone while the switch reads off, missing or unread", () => {
    const handle = fakeHandle();
    const speeches: ComposerDictation["speech"][] = [
      undefined,
      { enabled: false, has_api_key: true },
      { has_api_key: true },
      { enabled: true, has_api_key: true },
    ];
    for (const speech of speeches.slice(0, 3)) {
      const { unmount } = render(
        <Composer draft="" onDraftChange={() => {}} onSubmit={() => {}} dictation={{ speech, handle }} />
      );
      expect(screen.queryByTestId("assistant-mic")).not.toBeInTheDocument();
      expect(screen.queryByTestId("assistant-dictation-hint")).not.toBeInTheDocument();
      unmount();
    }
    const unsupported = fakeHandle({ supported: false });
    render(
      <Composer
        draft=""
        onDraftChange={() => {}}
        onSubmit={() => {}}
        dictation={{ speech: speeches[3], handle: unsupported }}
      />
    );
    expect(screen.queryByTestId("assistant-mic")).not.toBeInTheDocument();
  });

  it("abandons an in-progress capture when the cell hides", () => {
    const recording = fakeHandle({ status: "recording" });
    render(
      <Composer
        draft=""
        onDraftChange={() => {}}
        onSubmit={() => {}}
        dictation={{ speech: { enabled: false, has_api_key: true }, handle: recording }}
      />
    );
    expect(recording.cancel).toHaveBeenCalledTimes(1);
  });

  it("abandons a capture still waiting on the microphone when the cell hides", () => {
    const requesting = fakeHandle({ status: "requesting" });
    render(
      <Composer
        draft=""
        onDraftChange={() => {}}
        onSubmit={() => {}}
        dictation={{ speech: undefined, handle: requesting }}
      />
    );
    expect(requesting.cancel).toHaveBeenCalledTimes(1);
  });

  it("leaves a settled capture alone when the cell hides", () => {
    const idle = fakeHandle({ status: "idle" });
    render(
      <Composer draft="" onDraftChange={() => {}} onSubmit={() => {}} dictation={{ speech: undefined, handle: idle }} />
    );
    expect(idle.cancel).not.toHaveBeenCalled();
  });

  it("never cancels a capture the cell still shows", () => {
    const recording = fakeHandle({ status: "recording", elapsedSeconds: 4 });
    renderComposer({ dictation: keyedDictation(recording) });
    expect(recording.cancel).not.toHaveBeenCalled();
    expect(screen.getByTestId("assistant-mic")).toHaveAttribute("aria-label", "Release to transcribe");
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent(
      "Recording — release to transcribe · 0:04"
    );
  });

  it("offers a ready microphone with no hint when keyed and idle", () => {
    renderComposer({ dictation: keyedDictation(fakeHandle()) });
    const mic = screen.getByTestId("assistant-mic");
    expect(mic.tagName).toBe("BUTTON");
    expect(mic).toHaveAttribute("aria-label", "Hold to dictate");
    expect(mic).toHaveAttribute("aria-pressed", "false");
    expect(mic).toBeEnabled();
    expect(screen.queryByTestId("assistant-dictation-hint")).not.toBeInTheDocument();
  });

  it("shows no hint while the microphone prompt is still open", () => {
    renderComposer({ dictation: keyedDictation(fakeHandle({ status: "requesting" })) });
    expect(screen.getByTestId("assistant-mic")).toHaveAttribute("aria-label", "Hold to dictate");
    expect(screen.queryByTestId("assistant-dictation-hint")).not.toBeInTheDocument();
  });

  it("locks the microphone while an upload runs and says so", () => {
    renderComposer({ dictation: keyedDictation(fakeHandle({ status: "transcribing" })) });
    expect(screen.getByTestId("assistant-mic")).toBeDisabled();
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent("Transcribing…");
  });

  it("locks a keyed microphone with the composer and stays hintless", () => {
    renderComposer({ draft: "hello", configured: false, dictation: keyedDictation(fakeHandle()) });
    const mic = screen.getByTestId("assistant-mic");
    expect(mic.tagName).toBe("BUTTON");
    expect(mic).toHaveAttribute("aria-label", "Hold to dictate");
    expect(mic).toBeDisabled();
    expect(screen.queryByTestId("assistant-dictation-hint")).not.toBeInTheDocument();
  });

  it("explains a refused microphone", () => {
    renderComposer({ dictation: keyedDictation(fakeHandle({ status: "denied" })) });
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent(
      "Microphone access is blocked. Enable it in your browser's site settings, then try again."
    );
  });

  it("names missing hardware and failed capture distinctly", () => {
    const { unmount } = render(
      <Composer
        draft=""
        onDraftChange={() => {}}
        onSubmit={() => {}}
        dictation={keyedDictation(fakeHandle({ status: "error", captureFailure: "missing-hardware" }))}
      />
    );
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent("No microphone found.");
    unmount();
    render(
      <Composer
        draft=""
        onDraftChange={() => {}}
        onSubmit={() => {}}
        dictation={keyedDictation(fakeHandle({ status: "error", captureFailure: "capture-failed" }))}
      />
    );
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent("Couldn't access the microphone.");
  });

  it("reads a failed upload through its own message", () => {
    renderComposer({
      dictation: keyedDictation(fakeHandle({ status: "error", errorMessage: "provider down" })),
    });
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent("provider down");
  });

  it("falls back when a failed upload carries no message", () => {
    renderComposer({ dictation: keyedDictation(fakeHandle({ status: "error" })) });
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent("Voice input failed");
  });

  it("routes a keyless press to dictation settings with its hint", () => {
    renderComposer({
      dictation: { speech: { enabled: true, has_api_key: false }, handle: fakeHandle() },
    });
    const mic = screen.getByTestId("assistant-mic");
    expect(mic.tagName).toBe("A");
    expect(mic).toHaveAttribute("aria-label", "Set up voice dictation");
    expect(mic).toHaveAttribute("href", "/settings/profile/ai-assistant#voice-dictation");
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent("Voice dictation isn't set up.");
    const setup = screen.getByRole("link", { name: "Configure it" });
    expect(setup).toHaveAttribute("href", "/settings/profile/ai-assistant#voice-dictation");
  });

  it("keeps a keyless microphone shut while the composer locks", () => {
    renderComposer({
      draft: "hello",
      configured: false,
      dictation: { speech: { enabled: true, has_api_key: false }, handle: fakeHandle() },
    });
    const mic = screen.getByTestId("assistant-mic");
    expect(mic.tagName).toBe("BUTTON");
    expect(mic).toHaveAttribute("aria-label", "Set up voice dictation");
    expect(mic).toBeDisabled();
  });

  it("records on pointer hold and stops on release", () => {
    const handle = fakeHandle();
    renderComposer({ dictation: keyedDictation(handle) });
    const mic = screen.getByTestId("assistant-mic");
    fireEvent.pointerDown(mic);
    expect(handle.reset).toHaveBeenCalledTimes(1);
    expect(handle.start).toHaveBeenCalledTimes(1);
    fireEvent.pointerUp(mic);
    expect(handle.stop).toHaveBeenCalledTimes(1);
    fireEvent.pointerCancel(mic);
    expect(handle.cancel).toHaveBeenCalledTimes(1);
  });

  it("stops when the pointer slides off mid-recording only", () => {
    const recording = fakeHandle({ status: "recording" });
    const { unmount } = render(
      <Composer draft="" onDraftChange={() => {}} onSubmit={() => {}} dictation={keyedDictation(recording)} />
    );
    fireEvent.pointerLeave(screen.getByTestId("assistant-mic"));
    expect(recording.stop).toHaveBeenCalledTimes(1);
    unmount();
    const idle = fakeHandle({ status: "idle" });
    render(<Composer draft="" onDraftChange={() => {}} onSubmit={() => {}} dictation={keyedDictation(idle)} />);
    fireEvent.pointerLeave(screen.getByTestId("assistant-mic"));
    expect(idle.stop).not.toHaveBeenCalled();
  });

  it("records on keyboard hold and stops on release", () => {
    const handle = fakeHandle();
    renderComposer({ dictation: keyedDictation(handle) });
    const mic = screen.getByTestId("assistant-mic");
    expect(fireEvent.keyDown(mic, { key: " " })).toBe(false);
    expect(handle.start).toHaveBeenCalledTimes(1);
    expect(fireEvent.keyDown(mic, { key: "Enter", repeat: true })).toBe(true);
    expect(handle.start).toHaveBeenCalledTimes(1);
    expect(fireEvent.keyDown(mic, { key: "Tab" })).toBe(true);
    expect(handle.start).toHaveBeenCalledTimes(1);
    expect(fireEvent.keyUp(mic, { key: " " })).toBe(false);
    expect(handle.stop).toHaveBeenCalledTimes(1);
    expect(fireEvent.keyUp(mic, { key: "Tab" })).toBe(true);
    expect(handle.stop).toHaveBeenCalledTimes(1);
  });
});
