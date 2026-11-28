// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ChatInput } from "./ChatInput.js";

// The editor chunk is a dynamic import; under a full-suite load it can
// take seconds, so waits and the suite get explicit budgets (see
// RichTextEditor.test.tsx).
const LOAD_TIMEOUT = { timeout: 10_000 };
const EDITOR_TEST_TIMEOUT = 30_000;

function Harness({
  initial = "",
  onSubmit = () => {},
  disabled = false,
  onDraft,
}: {
  initial?: string;
  onSubmit?: (draft: string) => void;
  disabled?: boolean;
  onDraft?: (setDraft: (value: string) => void) => void;
}): React.ReactElement {
  const [draft, setDraft] = React.useState(initial);
  const draftRef = React.useRef(draft);
  draftRef.current = draft;
  React.useEffect(() => onDraft?.(setDraft), [onDraft]);
  return (
    <ChatInput
      label="Message"
      placeholder="Say something"
      value={draft}
      onChange={setDraft}
      onSubmit={() => onSubmit(draftRef.current)}
      disabled={disabled}
      maxHeight={200}
    />
  );
}

async function surface(): Promise<HTMLElement> {
  await vi.dynamicImportSettled();
  const box = await screen.findByRole("textbox", { name: "Message" }, LOAD_TIMEOUT);
  expect(box).toHaveAttribute("contenteditable", "true");
  return box;
}

function draftOf(box: HTMLElement): string | null {
  return box.closest("[data-chat-input]")?.getAttribute("data-draft") ?? null;
}

describe("ChatInput", { timeout: EDITOR_TEST_TIMEOUT }, () => {
  it("holds the box with an inert stand-in while the editor loads, then mounts a textbox", async () => {
    const { container } = render(<Harness initial="early words" />);
    // The stand-in paints the same frame immediately.
    expect(container.querySelector("[data-chat-input]")).not.toBeNull();
    const box = await surface();
    expect(box).toHaveAttribute("aria-multiline", "true");
    expect(box).toHaveAttribute("aria-placeholder", "Say something");
    expect(box).toHaveTextContent("early words");
    expect(box.closest("[data-chat-input]")).toHaveStyle({ maxHeight: "200px" });
  });

  it("shows the placeholder only while the draft is empty", async () => {
    render(<Harness />);
    const box = await surface();
    expect(screen.getByText("Say something")).toBeInTheDocument();
    box.focus();
    await userEvent.setup().keyboard("hi");
    await waitFor(() => expect(screen.queryByText("Say something")).not.toBeInTheDocument());
  });

  it("reports typed text as a plain string and submits on an unmodified Enter", async () => {
    const onSubmit = vi.fn();
    render(<Harness onSubmit={onSubmit} />);
    const box = await surface();
    box.focus();
    const user = userEvent.setup();
    await user.keyboard("Plan the sprint");
    await waitFor(() => expect(draftOf(box)).toBe("Plan the sprint"));
    await user.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith("Plan the sprint");
    // Enter never splits the draft into a second paragraph.
    expect(box.querySelectorAll("p")).toHaveLength(1);
  });

  it("breaks the line on Shift+Enter without submitting", async () => {
    const onSubmit = vi.fn();
    render(<Harness onSubmit={onSubmit} />);
    const box = await surface();
    box.focus();
    const user = userEvent.setup();
    await user.keyboard("one{Shift>}{Enter}{/Shift}two");
    await waitFor(() => expect(draftOf(box)).toBe("one\ntwo"));
    expect(onSubmit).not.toHaveBeenCalled();
  });

  // An IME Enter never reaches the handler: ProseMirror drops keydowns
  // while view.composing (and Safari's trailing Enter); chatInputText.test
  // pins the decision for the isComposing flag.
  it("does nothing on an Enter chord", async () => {
    const onSubmit = vi.fn();
    render(<Harness initial="draft" onSubmit={onSubmit} />);
    const box = await surface();
    for (const chord of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }]) {
      fireEvent.keyDown(box, { key: "Enter", ...chord });
    }
    expect(onSubmit).not.toHaveBeenCalled();
    expect(draftOf(box)).toBe("draft");
    // A chord inserts no hard break either (the keymap's Mod-Enter is swallowed).
    expect(box.querySelector("br:not(.ProseMirror-trailingBreak)")).toBeNull();
  });

  it("replaces the document when the draft changes from outside", async () => {
    let setDraft: (value: string) => void = () => {};
    render(<Harness initial="old" onDraft={(set) => (setDraft = set)} />);
    const box = await surface();
    act(() => setDraft("from a chip\nsecond line"));
    await waitFor(() => expect(box.innerText ?? box.textContent).toContain("from a chip"));
    expect(box.querySelectorAll("br:not(.ProseMirror-trailingBreak)")).toHaveLength(1);
    act(() => setDraft(""));
    await waitFor(() => expect(box).toHaveTextContent(""));
    expect(screen.getByText("Say something")).toBeInTheDocument();
  });

  it("locks when disabled and unlocks when re-enabled", async () => {
    const { rerender } = render(<Harness disabled />);
    await vi.dynamicImportSettled();
    const box = await screen.findByRole("textbox", { name: "Message" }, LOAD_TIMEOUT);
    await waitFor(() => expect(box).toHaveAttribute("contenteditable", "false"));
    expect(box).toHaveAttribute("aria-disabled", "true");
    rerender(<Harness />);
    await waitFor(() => expect(box).toHaveAttribute("contenteditable", "true"));
    expect(box).not.toHaveAttribute("aria-disabled");
  });
});
