// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  IssueTitleField,
  TITLE_AUTOSAVE_MS,
  TITLE_LABEL,
  TITLE_MAX_LENGTH,
  TITLE_PLACEHOLDER,
  TITLE_REQUIRED_MESSAGE,
} from "./IssueTitleField.js";
import { useSavePipeline } from "./savePipeline.js";

afterEach(() => {
  vi.useRealTimers();
});

function renderField(props: Partial<React.ComponentProps<typeof IssueTitleField>> = {}) {
  const onSave = vi.fn(async (_name: string) => {});
  function Host(): React.ReactElement {
    const pipeline = useSavePipeline();
    return (
      <>
        <IssueTitleField value="Fix login" disabled={false} pipeline={pipeline} onSave={onSave} {...props} />
        <p data-testid="pipeline-status">{pipeline.status}</p>
      </>
    );
  }
  const mounted = render(<Host />);
  return { onSave, ...mounted };
}

function status(): string {
  return screen.getByTestId("pipeline-status").textContent ?? "";
}

function field(): HTMLElement {
  return screen.getByLabelText(TITLE_LABEL);
}

describe("IssueTitleField", () => {
  it("renders the server name with its placeholder and cap", () => {
    renderField();
    const box = screen.getByRole("textbox", { name: TITLE_LABEL });
    expect(box).toHaveValue("Fix login");
    expect(box).toHaveAttribute("placeholder", TITLE_PLACEHOLDER);
    expect(box).toHaveAttribute("maxLength", String(TITLE_MAX_LENGTH));
  });

  it("renders a heading with no input when disabled", () => {
    renderField({ disabled: true });
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("saves 1.5s after typing stops", async () => {
    vi.useFakeTimers();
    const { onSave } = renderField();
    fireEvent.change(field(), { target: { value: "Fix login v2" } });
    expect(status()).toBe("saving");
    expect(onSave).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(TITLE_AUTOSAVE_MS);
    });
    expect(onSave).toHaveBeenCalledWith("Fix login v2");
    expect(status()).toBe("saved");
  });

  it("debounces across keystrokes into one save", () => {
    vi.useFakeTimers();
    const { onSave } = renderField();
    fireEvent.change(field(), { target: { value: "a" } });
    act(() => {
      vi.advanceTimersByTime(TITLE_AUTOSAVE_MS - 100);
    });
    fireEvent.change(field(), { target: { value: "ab" } });
    act(() => {
      vi.advanceTimersByTime(TITLE_AUTOSAVE_MS - 100);
    });
    expect(onSave).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith("ab");
  });

  it("saves trimmed text on blur", () => {
    vi.useFakeTimers();
    const { onSave } = renderField();
    fireEvent.change(field(), { target: { value: "  padded  " } });
    fireEvent.blur(field());
    expect(onSave).toHaveBeenCalledWith("padded");
    expect(field()).toHaveValue("padded");
  });

  it("reverts whitespace to the server value without saving", () => {
    vi.useFakeTimers();
    const { onSave } = renderField();
    fireEvent.change(field(), { target: { value: "   " } });
    fireEvent.blur(field());
    expect(onSave).not.toHaveBeenCalled();
    expect(field()).toHaveValue("Fix login");
    expect(status()).toBe("saved");
  });

  it("settles back to saved when edits revert to the server value", () => {
    vi.useFakeTimers();
    const { onSave } = renderField();
    fireEvent.change(field(), { target: { value: "changed" } });
    fireEvent.change(field(), { target: { value: "Fix login" } });
    act(() => {
      vi.advanceTimersByTime(TITLE_AUTOSAVE_MS);
    });
    expect(onSave).not.toHaveBeenCalled();
    expect(status()).toBe("saved");
  });

  it("ignores a blur with no edits", () => {
    vi.useFakeTimers();
    const { onSave } = renderField();
    fireEvent.focus(field());
    fireEvent.blur(field());
    expect(onSave).not.toHaveBeenCalled();
    expect(status()).toBe("saved");
  });

  it("saves dirty edits on unmount", () => {
    vi.useFakeTimers();
    const { onSave, unmount } = renderField();
    fireEvent.change(field(), { target: { value: "unmounted save" } });
    unmount();
    expect(onSave).toHaveBeenCalledWith("unmounted save");
  });

  it("saves nothing on a clean unmount", () => {
    vi.useFakeTimers();
    const { onSave, unmount } = renderField();
    unmount();
    expect(onSave).not.toHaveBeenCalled();
  });

  it("marks saved even when the write fails, keeping the text", async () => {
    vi.useFakeTimers();
    const onSave = vi.fn(async (_name: string) => {
      throw new Error("down");
    });
    function Host(): React.ReactElement {
      const pipeline = useSavePipeline();
      return (
        <>
          <IssueTitleField value="Fix login" disabled={false} pipeline={pipeline} onSave={onSave} />
          <p data-testid="pipeline-status">{pipeline.status}</p>
        </>
      );
    }
    render(<Host />);
    fireEvent.change(field(), { target: { value: "doomed" } });
    fireEvent.blur(field());
    await act(async () => {});
    expect(onSave).toHaveBeenCalledWith("doomed");
    expect(status()).toBe("saved");
    expect(field()).toHaveValue("doomed");
  });

  it("tracks the server value when it changes underneath", () => {
    vi.useFakeTimers();
    const onSave = vi.fn(async (_name: string) => {});
    function Host({ value }: { value: string }): React.ReactElement {
      const pipeline = useSavePipeline();
      return <IssueTitleField value={value} disabled={false} pipeline={pipeline} onSave={onSave} />;
    }
    const { rerender } = render(<Host value="Fix login" />);
    fireEvent.change(field(), { target: { value: "local edit" } });
    rerender(<Host value="Renamed elsewhere" />);
    expect(field()).toHaveValue("Renamed elsewhere");
    act(() => {
      vi.advanceTimersByTime(TITLE_AUTOSAVE_MS);
    });
    expect(onSave).not.toHaveBeenCalled();
  });

  it("shows a live counter while focused and the required note when empty", () => {
    renderField();
    expect(screen.queryByText(`9/${TITLE_MAX_LENGTH}`)).not.toBeInTheDocument();
    fireEvent.focus(field());
    expect(screen.getByText(`9/${TITLE_MAX_LENGTH}`)).toBeInTheDocument();
    fireEvent.change(field(), { target: { value: "" } });
    expect(screen.getByText(`0/${TITLE_MAX_LENGTH}`)).toHaveClass("text-(--danger)");
    expect(screen.getByText(TITLE_REQUIRED_MESSAGE)).toBeInTheDocument();
    expect(field()).toHaveAttribute("aria-invalid", "true");
    fireEvent.blur(field());
    expect(screen.queryByText(`0/${TITLE_MAX_LENGTH}`)).not.toBeInTheDocument();
  });
});
