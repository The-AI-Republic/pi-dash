// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  DESCRIPTION_ADD_LABEL,
  DESCRIPTION_EDIT_LABEL,
  DESCRIPTION_LABEL,
  IssueDescriptionField,
} from "./IssueDescriptionField.js";
import { useSavePipeline } from "./savePipeline.js";

function renderField(props: Partial<React.ComponentProps<typeof IssueDescriptionField>> = {}) {
  const onSave = vi.fn(async (_html: string) => {});
  function Host(hostProps: Partial<React.ComponentProps<typeof IssueDescriptionField>> = {}): React.ReactElement {
    const pipeline = useSavePipeline();
    return (
      <>
        <IssueDescriptionField
          value="<p>Stored words</p>"
          disabled={false}
          pipeline={pipeline}
          onSave={onSave}
          {...props}
          {...hostProps}
        />
        <p data-testid="pipeline-status">{pipeline.status}</p>
      </>
    );
  }
  const mounted = render(<Host />);
  return { onSave, Host, ...mounted };
}

function status(): string {
  return screen.getByTestId("pipeline-status").textContent ?? "";
}

async function openEditor(): Promise<HTMLElement> {
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: DESCRIPTION_EDIT_LABEL }));
  return screen.findByLabelText(DESCRIPTION_LABEL);
}

describe("IssueDescriptionField", () => {
  it("reads stored HTML until the surface opens for editing", async () => {
    renderField();
    expect(screen.getByRole("region", { name: DESCRIPTION_LABEL })).toHaveTextContent("Stored words");
    const editable = await openEditor();
    expect(editable).toHaveAttribute("contenteditable", "true");
    expect(editable).toHaveTextContent("Stored words");
  });

  it("opens from Enter and ignores other keys", async () => {
    const user = userEvent.setup();
    const { container } = renderField();
    const surface = screen.getByRole("button", { name: DESCRIPTION_EDIT_LABEL });
    surface.focus();
    await user.keyboard("x");
    // The view region shares the Description label; the editable element
    // is what distinguishes the open editor.
    expect(container.querySelector("[contenteditable='true']")).toBeNull();
    await user.keyboard("{Enter}");
    expect(await screen.findByLabelText(DESCRIPTION_LABEL)).toBeInTheDocument();
  });

  it("opens from Space", async () => {
    const user = userEvent.setup();
    renderField();
    screen.getByRole("button", { name: DESCRIPTION_EDIT_LABEL }).focus();
    await user.keyboard(" ");
    expect(await screen.findByLabelText(DESCRIPTION_LABEL)).toBeInTheDocument();
  });

  it("offers an add button when empty", async () => {
    const user = userEvent.setup();
    renderField({ value: "<p></p>" });
    await user.click(screen.getByRole("button", { name: DESCRIPTION_ADD_LABEL }));
    expect(await screen.findByLabelText(DESCRIPTION_LABEL)).toBeInTheDocument();
  });

  it("leaves stored links working instead of opening", async () => {
    const user = userEvent.setup();
    const { container } = renderField({ value: '<p>See <a href="#example">example</a></p>' });
    await user.click(screen.getByRole("link", { name: "example" }));
    expect(container.querySelector("[contenteditable='true']")).toBeNull();
  });

  it("saves typed edits on blur", async () => {
    const user = userEvent.setup();
    const { onSave } = renderField();
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" plus more");
    expect(status()).toBe("saving");
    fireEvent.blur(editable);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    const html = onSave.mock.calls[0]?.[0] as string;
    expect(html).toContain("Stored words");
    expect(html).toContain("plus more");
    await waitFor(() => expect(status()).toBe("saved"));
  });

  it("does not save text the user typed and then deleted", async () => {
    const user = userEvent.setup();
    const { onSave } = renderField();
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" plus");
    await user.keyboard("{Backspace}{Backspace}{Backspace}{Backspace}{Backspace}");
    fireEvent.blur(editable);
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("autosaves after standing idle", async () => {
    const user = userEvent.setup();
    const { onSave } = renderField();
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" idle save");
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1), { timeout: 5000 });
    expect((onSave.mock.calls[0]?.[0] as string) ?? "").toContain("idle save");
  }, 10000);

  it("schedules nothing for opening the editor alone", async () => {
    const { onSave } = renderField();
    await openEditor();
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(onSave).not.toHaveBeenCalled();
    expect(status()).toBe("saved");
  });

  it("saves dirty edits on unmount and nothing when clean", async () => {
    const user = userEvent.setup();
    const { onSave, unmount } = renderField();
    const editable = await openEditor();
    editable.focus();
    await user.keyboard("unmounted");
    unmount();
    expect(onSave).toHaveBeenCalledTimes(1);
    expect((onSave.mock.calls[0]?.[0] as string) ?? "").toContain("unmounted");
  });

  it("keeps failed edits dirty and retries them", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn(async (_html: string) => {});
    onSave.mockRejectedValueOnce(new Error("down"));
    function Host(): React.ReactElement {
      const pipeline = useSavePipeline();
      return (
        <>
          <IssueDescriptionField value="<p>base</p>" disabled={false} pipeline={pipeline} onSave={onSave} />
          <p data-testid="pipeline-status">{pipeline.status}</p>
        </>
      );
    }
    render(<Host />);
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" retry me");
    fireEvent.blur(editable);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(status()).toBe("saved"));
    // Still dirty from the failure: the next blur retries the same HTML.
    editable.focus();
    fireEvent.blur(editable);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
  });

  it("re-schedules a failed save when the server moves", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn(async (_html: string) => {});
    onSave.mockRejectedValueOnce(new Error("down"));
    function Host({ value }: { value: string }): React.ReactElement {
      const pipeline = useSavePipeline();
      return <IssueDescriptionField value={value} disabled={false} pipeline={pipeline} onSave={onSave} />;
    }
    const { rerender } = render(<Host value="<p>base</p>" />);
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" retry me");
    fireEvent.blur(editable);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    rerender(<Host value="<p>external</p>" />);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2), { timeout: 5000 });
    expect((onSave.mock.calls[1]?.[0] as string) ?? "").toContain("retry me");
  }, 10000);

  it("converges when the server confirms the save", async () => {
    const user = userEvent.setup();
    const { onSave, rerender, Host } = renderField();
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" confirmed");
    fireEvent.blur(editable);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    const savedHtml = onSave.mock.calls[0]?.[0] as string;
    rerender(<Host value={savedHtml} />);
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("re-saves newer edits after an external value lands", async () => {
    const user = userEvent.setup();
    const { onSave, rerender, Host } = renderField({ value: "<p>base</p>" });
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" mine");
    // An external write lands while the debounce is still pending: the
    // pending save still fires once, carrying the newer local HTML.
    rerender(<Host value="<p>external</p>" />);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1), { timeout: 5000 });
    expect((onSave.mock.calls[0]?.[0] as string) ?? "").toContain("mine");
  }, 10000);

  it("shows a restore at once and closes the editor", async () => {
    const { container, rerender, Host } = renderField({ value: "<p>current</p>" });
    await openEditor();
    expect(container.querySelector("[contenteditable='true']")).not.toBeNull();
    rerender(<Host value="<p>current</p>" restored={{ html: "<p>version two</p>", token: 1 }} />);
    expect(container.querySelector("[contenteditable='true']")).toBeNull();
    expect(screen.getByRole("region", { name: DESCRIPTION_LABEL })).toHaveTextContent("version two");
  });

  it("renders read-only when disabled and nothing when empty", () => {
    const { rerender, Host } = renderField({ disabled: true });
    expect(screen.getByRole("region", { name: DESCRIPTION_LABEL })).toHaveTextContent("Stored words");
    expect(screen.queryByRole("button", { name: DESCRIPTION_EDIT_LABEL })).not.toBeInTheDocument();
    rerender(<Host disabled value="<p></p>" />);
    expect(screen.queryByRole("region", { name: DESCRIPTION_LABEL })).not.toBeInTheDocument();
  });

  it("drops pending edits when disabled mid-edit", async () => {
    const user = userEvent.setup();
    const { container, onSave, rerender, Host } = renderField();
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" dropped");
    rerender(<Host disabled />);
    expect(container.querySelector("[contenteditable='true']")).toBeNull();
    expect(onSave).not.toHaveBeenCalled();
  });

  it("persists edits typed while a save is in flight", async () => {
    const user = userEvent.setup();
    let resolveSave!: () => void;
    const gate = new Promise<void>((resolve) => {
      resolveSave = resolve;
    });
    const onSave = vi.fn(async (_html: string) => {});
    onSave.mockImplementationOnce(() => gate);
    function Host(): React.ReactElement {
      const pipeline = useSavePipeline();
      return (
        <>
          <IssueDescriptionField value="<p>base</p>" disabled={false} pipeline={pipeline} onSave={onSave} />
          <p data-testid="pipeline-status">{pipeline.status}</p>
        </>
      );
    }
    render(<Host />);
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" first");
    fireEvent.blur(editable);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    // More keystrokes land while that save is still in flight.
    editable.focus();
    await user.keyboard(" second");
    resolveSave();
    // No further input: the trailing edits go out on their own.
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
    expect((onSave.mock.calls[1]?.[0] as string) ?? "").toContain("second");
    await waitFor(() => expect(status()).toBe("saved"));
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(onSave).toHaveBeenCalledTimes(2);
  });

  it("does not double-send on overlapping flushes", async () => {
    const user = userEvent.setup();
    let resolveSave!: () => void;
    const gate = new Promise<void>((resolve) => {
      resolveSave = resolve;
    });
    const onSave = vi.fn(async (_html: string) => {});
    onSave.mockImplementationOnce(() => gate);
    function Host(): React.ReactElement {
      const pipeline = useSavePipeline();
      return (
        <>
          <IssueDescriptionField value="<p>base</p>" disabled={false} pipeline={pipeline} onSave={onSave} />
          <p data-testid="pipeline-status">{pipeline.status}</p>
        </>
      );
    }
    render(<Host />);
    const editable = await openEditor();
    editable.focus();
    await user.keyboard(" once");
    fireEvent.blur(editable);
    fireEvent.blur(editable);
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(onSave).toHaveBeenCalledTimes(1);
    resolveSave();
    await waitFor(() => expect(status()).toBe("saved"));
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(onSave).toHaveBeenCalledTimes(1);
  });
});
