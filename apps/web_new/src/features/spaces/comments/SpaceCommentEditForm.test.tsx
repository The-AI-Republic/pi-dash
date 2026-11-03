// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceComment, Transport, TransportInit } from "@pidash/api-client";
import { createClient } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown }));

vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));
vi.mock("../lib/spaceNotices.js", () => ({
  notifySpaceAttachmentFailure: vi.fn(),
}));

// The Tiptap chunk is lazy; the form's own behaviors are what this
// suite pins, and unit tests elsewhere cover the editor surface.
vi.mock("../../../shared/editor/RichTextEditor.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/RichTextEditor.js")>();
  function FakeEditor({
    initialHtml,
    onChange,
    label,
    onReady,
    onMentionQuery,
    handleKeyDown,
  }: {
    initialHtml: string;
    onChange: (html: string) => void;
    label: string;
    onReady?: (handle: {
      insertImage: (src: string) => void;
      insertMention: (target: { id: string; name: string }) => void;
      getHtml: () => string;
    }) => void;
    onMentionQuery?: (query: string | null) => void;
    handleKeyDown?: (event: KeyboardEvent) => boolean;
  }) {
    const images = React.useRef<string[]>([]);
    const mentions = React.useRef<{ id: string; name: string }[]>([]);
    const boxRef = React.useRef<HTMLTextAreaElement>(null);
    const [, rerender] = React.useReducer((value: number) => value + 1, 0);
    const text = React.useRef(initialHtml.replace(/<[^>]*>/g, "").trim());
    const htmlRef = React.useRef(initialHtml);
    const emit = (): void => {
      const markup =
        images.current.map((src) => `<img src="${src}">`).join("") +
        mentions.current
          .map(
            (mention) =>
              `<mention-component id="${mention.id}" entity_identifier="${mention.id}" entity_name="user_mention" data-name="${mention.name}"></mention-component>`
          )
          .join("");
      const nextHtml = text.current.trim() === "" && markup === "" ? "<p></p>" : `<p>${text.current}${markup}</p>`;
      htmlRef.current = nextHtml;
      onChange(nextHtml);
      onMentionQuery?.(mentionQueryAt(text.current)?.text ?? null);
    };
    React.useEffect(
      () =>
        onReady?.({
          insertImage: (src) => {
            images.current.push(src);
            rerender();
            emit();
          },
          insertMention: (target) => {
            const query = mentionQueryAt(text.current);
            if (query !== null) text.current = text.current.slice(0, text.current.length - query.text.length - 1);
            if (boxRef.current !== null) boxRef.current.value = text.current;
            mentions.current.push(target);
            rerender();
            emit();
          },
          // The live document, as the real handle reads it at key time.
          getHtml: () => htmlRef.current,
        }),
      [onReady]
    );
    return (
      <div>
        {images.current.map((src) => (
          <img key={src} src={src} alt="" />
        ))}
        <textarea
          ref={boxRef}
          aria-label={label}
          defaultValue={text.current}
          onInput={(event) => {
            text.current = event.currentTarget.value;
            emit();
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) event.preventDefault();
            else if (handleKeyDown?.(event.nativeEvent) === true) event.preventDefault();
          }}
        />
      </div>
    );
  }
  return { ...original, RichTextEditor: FakeEditor, CommentRichTextEditor: FakeEditor };
});

import { mentionQueryAt } from "../../../shared/editor/mention.js";
import { notifySpaceAttachmentFailure } from "../lib/spaceNotices.js";
import { SpaceCommentEditForm } from "./SpaceCommentEditForm.js";

const ANCHOR = "anchor-1";
const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const COMMENT_ID = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "523e4567-e89b-12d3-a456-426614174004";

function comment(overrides: Partial<SpaceComment> = {}): SpaceComment {
  return {
    id: COMMENT_ID,
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
    issue: ISSUE_ID,
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    comment_html: "<p>Hello</p>",
    comment_stripped: "Hello",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "EXTERNAL",
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

const GRANT = {
  asset_id: "asset-1",
  asset_url: null,
  upload_data: { url: "https://storage.test/upload", fields: { key: "k1" } },
};

const MEMBERS = [
  { id: "723e4567-e89b-12d3-a456-426614174006", member: ADA, member__display_name: "Ada L", member__avatar: "" },
];

type HandlerAnswer = { status: number; body: unknown };

function renderForm(
  props: Partial<React.ComponentProps<typeof SpaceCommentEditForm>> = {},
  handler: (call: { method: string; url: string; body: string }) => HandlerAnswer | Promise<HandlerAnswer> = () => ({
    status: 200,
    body: [],
  })
) {
  const calls: { method: string; url: string; body: string }[] = [];
  const transport: Transport = async (url: string, init?: TransportInit) => {
    const call = { method: init?.method ?? "GET", url, body: init?.body ?? "" };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = await handler(call);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  };
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  vi.stubGlobal("fetch", async () => ({ ok: true, status: 204 }));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const onSave = props.onSave ?? vi.fn(async () => undefined);
  const onCancel = props.onCancel ?? vi.fn();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SpaceCommentEditForm
        anchor={ANCHOR}
        comment={comment()}
        resolveAsset={(src) => src}
        onSave={onSave}
        onCancel={onCancel}
        {...props}
      />
    </QueryClientProvider>
  );
  return { calls, onSave, onCancel, unmount: view.unmount };
}

async function editor(): Promise<HTMLElement> {
  let box: HTMLElement | null = null;
  await waitFor(() => {
    box = document.querySelector('textarea[aria-label="Edit comment"]');
    expect(box).not.toBeNull();
  });
  return box as unknown as HTMLElement;
}

function saveButton(): HTMLElement {
  return screen.getByRole("button", { name: "Save comment" });
}

function cancelButton(): HTMLElement {
  return screen.getByRole("button", { name: "Cancel editing" });
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.mocked(notifySpaceAttachmentFailure).mockClear();
});

describe("SpaceCommentEditForm", () => {
  it("opens prefilled and saves the edited body on Enter (SPC-056)", async () => {
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const box = await editor();
    expect(box).toHaveValue("Hello");
    expect(saveButton()).toBeDisabled();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" edited");
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onSave).toHaveBeenCalledWith("<p>Hello edited</p>"));
  });

  it("saves on a native form submit (SPC-056)", async () => {
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const box = await editor();
    box.focus();
    await user.keyboard("{End} edited");
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    fireEvent.submit(screen.getByTestId("space-comment-edit"));
    await waitFor(() => expect(onSave).toHaveBeenCalledWith("<p>Hello edited</p>"));
  });

  it("saves through the check button too (SPC-056)", async () => {
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" edited");
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    await user.click(saveButton());
    await waitFor(() => expect(onSave).toHaveBeenCalledWith("<p>Hello edited</p>"));
  });

  it("saves nothing while pristine or empty (SPC-056)", async () => {
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard("{Enter}");
    await user.tripleClick(box);
    await user.keyboard("{Backspace}");
    await waitFor(() => expect(saveButton()).toBeDisabled());
    await user.keyboard("{Enter}");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("cancels without a write (SPC-056)", async () => {
    const user = userEvent.setup();
    const { onSave, onCancel } = renderForm();
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" edited");
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    await user.click(cancelButton());
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("disables cancel while the save is out (SPC-056)", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const onSave = vi.fn(() => gate);
    renderForm({ onSave });
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" edited");
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(cancelButton()).toBeDisabled());
    release();
    await waitFor(() => expect(cancelButton()).not.toBeDisabled());
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("keeps the draft for a retry when the save is refused (SPC-056)", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn(async () => {
      throw new Error("refused");
    });
    renderForm({ onSave });
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" edited");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    expect(box).toHaveValue("Hello edited");
    expect(screen.getByTestId("space-comment-edit")).toBeInTheDocument();
  });

  it("binds edit uploads to the comment and inserts the node (SPC-053)", async () => {
    const user = userEvent.setup();
    const { calls } = renderForm({}, (call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      if (call.method === "PATCH") return { status: 204, body: "" };
      return { status: 200, body: [] };
    });
    await editor();
    const input = screen.getByTestId("space-comment-edit-image");
    await user.upload(input, new File(["bytes"], "pic.png", { type: "image/png" }));
    await waitFor(() => expect(screen.getByTestId("space-comment-edit").querySelector("img")).not.toBeNull());
    const grantCall = calls.find((call) => call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/"));
    expect(JSON.parse(grantCall?.body ?? "{}")).toMatchObject({ entity_identifier: COMMENT_ID });
    expect(notifySpaceAttachmentFailure).not.toHaveBeenCalled();
  });

  it("reports a refused edit upload and keeps the draft (SPC-053)", async () => {
    const user = userEvent.setup();
    renderForm({}, (call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      return { status: 200, body: [] };
    });
    vi.stubGlobal("fetch", async () => ({ ok: false, status: 500 }));
    const box = await editor();
    const input = screen.getByTestId("space-comment-edit-image");
    await user.upload(input, new File(["bytes"], "pic.png", { type: "image/png" }));
    await waitFor(() => expect(notifySpaceAttachmentFailure).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("space-comment-edit").querySelector("img")).toBeNull();
    expect(box).toHaveValue("Hello");
  });

  it("ignores an empty file choice (SPC-053)", async () => {
    const { calls } = renderForm();
    await editor();
    fireEvent.change(screen.getByTestId("space-comment-edit-image"), { target: { files: [] } });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls.some((call) => call.url.includes("/assets/v2/"))).toBe(false);
  });

  it("tolerates an unmount mid-upload (SPC-053)", async () => {
    const user = userEvent.setup();
    let release!: (value: { ok: boolean; status: number }) => void;
    const gate = new Promise<{ ok: boolean; status: number }>((resolve) => {
      release = resolve;
    });
    const { unmount } = renderForm({}, (call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      if (call.method === "PATCH") return { status: 204, body: "" };
      return { status: 200, body: [] };
    });
    vi.stubGlobal("fetch", () => gate);
    await editor();
    await user.upload(
      screen.getByTestId("space-comment-edit-image"),
      new File(["bytes"], "pic.png", { type: "image/png" })
    );
    unmount();
    release({ ok: true, status: 204 });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(notifySpaceAttachmentFailure).not.toHaveBeenCalled();
  });

  it("holds Enter while an upload is out (SPC-053)", async () => {
    const user = userEvent.setup();
    let release!: (value: { ok: boolean; status: number }) => void;
    const gate = new Promise<{ ok: boolean; status: number }>((resolve) => {
      release = resolve;
    });
    const { onSave } = renderForm({}, (call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      if (call.method === "PATCH") return { status: 204, body: "" };
      return { status: 200, body: [] };
    });
    vi.stubGlobal("fetch", () => gate);
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" edited");
    await user.upload(
      screen.getByTestId("space-comment-edit-image"),
      new File(["bytes"], "pic.png", { type: "image/png" })
    );
    await waitFor(() => expect(saveButton()).toBeDisabled());
    box.focus();
    await user.keyboard("{Enter}");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onSave).not.toHaveBeenCalled();
    release({ ok: true, status: 204 });
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    box.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  });

  it("mentions a board member while editing (SPC-054)", async () => {
    const user = userEvent.setup();
    const { onSave } = renderForm({}, (call) => {
      if (call.url.endsWith("/members/")) return { status: 200, body: MEMBERS };
      return { status: 200, body: [] };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" @ad");
    await screen.findByRole("list", { name: "Mention suggestions" });
    await user.keyboard("{Enter}");
    await waitFor(() => expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(String(vi.mocked(onSave).mock.calls[0]?.[0])).toContain("mention-component");
  });

  it("dismisses the mention popup on Escape and keeps the form on a bare Escape (SPC-054)", async () => {
    const user = userEvent.setup();
    renderForm({}, (call) => {
      if (call.url.endsWith("/members/")) return { status: 200, body: MEMBERS };
      return { status: 200, body: [] };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" @ad");
    await screen.findByRole("list", { name: "Mention suggestions" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument());
    await user.keyboard("{Escape}");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.getByTestId("space-comment-edit")).toBeInTheDocument();
  });

  it("newlines on Shift+Enter and leaves Ctrl/Cmd+Enter inert (SPC-056)", async () => {
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard("one");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    await user.keyboard("{Control>}{Enter}{/Control}");
    await user.keyboard("{Meta>}{Enter}{/Meta}");
    await user.keyboard("two");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onSave).not.toHaveBeenCalled();
    expect(box).toHaveValue("Helloone\ntwo");
  });

  it("leaves a composing Enter to the input (SPC-056)", async () => {
    const user = userEvent.setup();
    const { onSave } = renderForm();
    const box = await editor();
    box.focus();
    await user.keyboard("{End}");
    await user.keyboard(" edited");
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    const composing = new KeyboardEvent("keydown", { key: "Enter", bubbles: true });
    Object.defineProperty(composing, "isComposing", { value: true });
    fireEvent(box, composing);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onSave).not.toHaveBeenCalled();
  });
});
