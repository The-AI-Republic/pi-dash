// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueComment, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { CommentEditForm } from "./CommentEditForm.js";

// The lazy Tiptap chunk loads once per worker and is slow under the full
// suite; the form's own behaviors are what this file pins.
vi.mock("../../../shared/editor/RichTextEditor.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/RichTextEditor.js")>();
  function FakeEditor({
    initialHtml,
    onChange,
    label,
    autoFocus,
    onReady,
  }: {
    initialHtml: string;
    onChange: (html: string) => void;
    label: string;
    autoFocus?: boolean;
    onReady?: (handle: { insertImage: (src: string) => void }) => void;
  }) {
    const images = React.useRef<string[]>([]);
    const [, rerender] = React.useReducer((value: number) => value + 1, 0);
    React.useEffect(
      () =>
        onReady?.({
          insertImage: (src) => {
            images.current.push(src);
            rerender();
          },
        }),
      [onReady]
    );
    return (
      <div>
        {images.current.map((src) => (
          <img key={src} src={src} alt="" />
        ))}
        <textarea
          aria-label={label}
          autoFocus={autoFocus}
          defaultValue={initialHtml
            .replace(/<[^>]*>/g, "")
            .replace(/\s+/g, " ")
            .trim()}
          onInput={(event) => onChange(`<p>${event.currentTarget.value}</p>`)}
        />
      </div>
    );
  }
  return { ...original, RichTextEditor: FakeEditor, CommentRichTextEditor: FakeEditor };
});

const ADA = "523e4567-e89b-12d3-a456-426614174004";

function comment(overrides: Partial<IssueComment> = {}): IssueComment {
  return {
    id: "823e4567-e89b-12d3-a456-426614174008",
    issue: "223e4567-e89b-12d3-a456-426614174001",
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: ADA,
    actor_detail: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    comment_html: "<p>Stored words</p>",
    comment_stripped: "Stored words",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "INTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: "2026-10-16T22:00:00Z",
    updated_at: "2026-10-16T22:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function renderForm(props: Partial<React.ComponentProps<typeof CommentEditForm>> = {}) {
  const onSave = vi.fn(async () => undefined);
  const onCancel = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CommentEditForm
        workspaceSlug="acme"
        projectId="p-1"
        comment={comment()}
        onSave={onSave}
        onCancel={onCancel}
        {...props}
      />
    </QueryClientProvider>
  );
  return {
    onSave: (props.onSave as typeof onSave | undefined) ?? onSave,
    onCancel: (props.onCancel as typeof onCancel | undefined) ?? onCancel,
  };
}

function stubTransport(
  handler: (method: string, url: string) => { status: number; body: unknown } = () => ({ status: 200, body: {} })
) {
  const calls: { method: string; url: string }[] = [];
  createAppClient("https://api.test", async (url: string, init: TransportInit) => {
    calls.push({ method: init.method, url });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = handler(init.method, url);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  });
  return calls;
}

async function surface(text?: string): Promise<HTMLTextAreaElement> {
  const editable = (await screen.findByLabelText("Edit comment")) as HTMLTextAreaElement;
  if (text !== undefined) expect(editable.value).toBe(text);
  return editable;
}

/** Put the caret at the end, as an editor would after opening. */
function caretToEnd(editable: HTMLTextAreaElement): void {
  editable.focus();
  editable.setSelectionRange(editable.value.length, editable.value.length);
}

afterEach(() => {
  vi.unstubAllGlobals();
  resetAppClient();
  vi.mocked(notifyError).mockClear();
});

// The lazy Tiptap chunk loads once per worker; under the full suite
// that import can outlast the default per-test timeout.
describe("CommentEditForm", () => {
  it("opens prefilled and focused on the stored body", async () => {
    stubTransport();
    renderForm();
    const editable = await surface();
    await waitFor(() => expect(document.activeElement).toBe(editable));
    expect(editable.value).toBe("Stored words");
  });

  it("saves the edited body from the check button", async () => {
    stubTransport();
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const editable = await surface("Stored words");
    caretToEnd(editable);
    await user.keyboard(" now");
    await waitFor(() => expect(editable.value).toMatch(/now/));
    await user.click(screen.getByRole("button", { name: "Save comment" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledWith(expect.stringContaining("now"));
  });

  it("saves on Enter and leaves modifier-plus-Enter for a line break", async () => {
    stubTransport();
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const editable = await surface("Stored words");
    caretToEnd(editable);
    await user.keyboard(" one");
    await waitFor(() => expect(editable.value).toMatch(/one/));
    await user.keyboard("{Shift>}{Enter}{/Shift}two");
    expect(onSave).not.toHaveBeenCalled();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  });

  it("blocks an empty save", async () => {
    stubTransport();
    const user = userEvent.setup();
    const { onSave } = renderForm({ comment: comment({ comment_html: "<p></p>" }) });
    await surface("");
    expect(screen.getByRole("button", { name: "Save comment" })).toBeDisabled();
    await user.keyboard("{Enter}");
    fireEvent.submit(screen.getByRole("button", { name: "Save comment" }).closest("form")!);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("leaves save disabled until the draft differs from the stored body", async () => {
    stubTransport();
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const editable = await surface("Stored words");
    expect(screen.getByRole("button", { name: "Save comment" })).toBeDisabled();
    // A bare Enter or submit with no content change saves nothing.
    fireEvent.keyDown(editable, { key: "Enter" });
    fireEvent.submit(screen.getByRole("button", { name: "Save comment" }).closest("form")!);
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Save comment" })).toBeDisabled();
    caretToEnd(editable);
    await user.keyboard(" edited");
    await waitFor(() => expect(screen.getByRole("button", { name: "Save comment" })).not.toBeDisabled());
    fireEvent.input(editable, { target: { value: "Stored words" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Save comment" })).toBeDisabled());
    expect(onSave).not.toHaveBeenCalled();
  });

  it("keeps the draft and re-enables the check when the save is refused", async () => {
    stubTransport();
    const user = userEvent.setup();
    const onSave = vi.fn(async () => Promise.reject(new Error("nope")));
    renderForm({ onSave });
    const editable = await surface("Stored words");
    caretToEnd(editable);
    await user.keyboard(" stays");
    await waitFor(() => expect(editable.value).toMatch(/stays/));
    await user.click(screen.getByRole("button", { name: "Save comment" }));
    await waitFor(() => expect(onSave).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByRole("button", { name: "Save comment" })).not.toBeDisabled());
    expect(editable.value).toMatch(/stays/);
  });

  it("cancels without saving", async () => {
    stubTransport();
    const user = userEvent.setup();
    const { onSave, onCancel } = renderForm();
    const editable = await surface("Stored words");
    caretToEnd(editable);
    await user.keyboard(" discarded");
    await user.click(screen.getByRole("button", { name: "Cancel editing" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("uploads an image, attaches it to the comment and embeds it", async () => {
    const calls = stubTransport((method) =>
      method === "POST"
        ? {
            status: 200,
            body: {
              asset_id: "asset-7",
              asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-7/",
              upload_data: { url: "https://s3.test/upload", fields: {} },
            },
          }
        : { status: 204, body: "" }
    );
    vi.stubGlobal("fetch", async () => ({ ok: true, status: 204 }));
    renderForm();
    await surface();
    fireEvent.change(screen.getByLabelText("Add an image to the comment"), {
      target: { files: [new File(["img"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() =>
      expect(document.querySelector("img")?.getAttribute("src")).toBe(
        "/api/assets/v2/workspaces/acme/projects/p-1/asset-7/"
      )
    );
    expect(calls.some((call) => call.url.includes(`/projects/p-1/${comment().id}/bulk/`))).toBe(true);
  });

  it("ignores a dismissed picker and notices a refused image", async () => {
    stubTransport((_method, url) =>
      url.includes("/bulk/") ? { status: 204, body: "" } : { status: 400, body: { error: "Invalid file type." } }
    );
    renderForm();
    await surface();
    const input = screen.getByLabelText("Add an image to the comment");
    fireEvent.change(input, { target: { files: [] } });
    fireEvent.change(input, { target: { files: [new File(["x"], "bad.svg", { type: "image/svg+xml" })] } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't upload the image", "Invalid file type."));
  });

  it("blocks an empty submit and saves only once while a save is out", async () => {
    const user = userEvent.setup();
    let release: (() => void) | null = null;
    const onSave = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    stubTransport();
    renderForm({ onSave });
    const box = await screen.findByLabelText("Edit comment");
    const form = screen.getByRole("button", { name: "Save comment" }).closest("form")!;
    fireEvent.input(box, { target: { value: "" } });
    fireEvent.submit(form);
    expect(onSave).not.toHaveBeenCalled();
    fireEvent.input(box, { target: { value: "Once" } });
    await user.click(screen.getByRole("button", { name: "Save comment" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    fireEvent.submit(form);
    expect(onSave).toHaveBeenCalledTimes(1);
    (release as (() => void) | null)?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "Save comment" })).not.toBeDisabled());
  });

  it("notices an image that uploads but cannot attach while editing", async () => {
    stubTransport((_method, url) => {
      if (url.includes("/bulk/")) return { status: 500, body: { error: "Asset missing" } };
      if (url.includes("/assets/v2/")) {
        return {
          status: 200,
          body: {
            asset_id: "asset-3",
            asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-3/",
            upload_data: { url: "https://s3.test/u", fields: {} },
          },
        };
      }
      return { status: 204, body: "" };
    });
    vi.stubGlobal("fetch", async () => ({ ok: true, status: 204 }));
    renderForm();
    await surface();
    fireEvent.change(screen.getByLabelText("Add an image to the comment"), {
      target: { files: [new File(["x"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't finish the comment upload", "Asset missing")
    );
  });
}, 20_000);
