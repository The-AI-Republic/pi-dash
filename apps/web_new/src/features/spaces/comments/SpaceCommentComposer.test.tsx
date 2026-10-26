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
  notifySpaceCommentFailure: vi.fn(),
}));

// The Tiptap chunk is lazy; the composer's own behaviors are what this
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
    }) => void;
    onMentionQuery?: (query: string | null) => void;
    handleKeyDown?: (event: KeyboardEvent) => boolean;
  }) {
    const images = React.useRef<string[]>([]);
    const mentions = React.useRef<{ id: string; name: string }[]>([]);
    const boxRef = React.useRef<HTMLTextAreaElement>(null);
    const [, rerender] = React.useReducer((value: number) => value + 1, 0);
    const text = React.useRef("");
    const emit = (): void => {
      const markup =
        images.current.map((src) => `<img src="${src}">`).join("") +
        mentions.current
          .map(
            (mention) =>
              `<mention-component id="${mention.id}" entity_identifier="${mention.id}" entity_name="user_mention" data-name="${mention.name}"></mention-component>`
          )
          .join("");
      onChange(text.current.trim() === "" && markup === "" ? "<p></p>" : `<p>${text.current}${markup}</p>`);
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
            // The real handle replaces the typed @query with the node.
            const query = mentionQueryAt(text.current);
            if (query !== null) text.current = text.current.slice(0, text.current.length - query.text.length - 1);
            if (boxRef.current !== null) boxRef.current.value = text.current;
            mentions.current.push(target);
            rerender();
            emit();
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
          ref={boxRef}
          aria-label={label}
          defaultValue={initialHtml.replace(/<[^>]*>/g, "").trim()}
          onInput={(event) => {
            text.current = event.currentTarget.value;
            emit();
          }}
          onKeyDown={(event) => {
            // Mirror the editing surface: the editor-level guard runs
            // first, and a swallowed key reaches the form prevented,
            // exactly as ProseMirror delivers it (SPC-053). The real
            // surface inserts nothing on Ctrl/Cmd+Enter; a raw textarea
            // would newline instead, so the fake swallows that default.
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
import { notifySpaceAttachmentFailure, notifySpaceCommentFailure } from "../lib/spaceNotices.js";
import { spaceKeys } from "../api/keys.js";
import { SpaceCommentComposer } from "./SpaceCommentComposer.js";

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

function renderComposer(
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
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SpaceCommentComposer anchor={ANCHOR} issueId={ISSUE_ID} />
    </QueryClientProvider>
  );
  return { calls, queryClient, unmount: view.unmount };
}

async function editor(): Promise<HTMLElement> {
  let box: HTMLElement | null = null;
  await waitFor(() => {
    box = document.querySelector('textarea[aria-label="Write a comment"]');
    expect(box).not.toBeNull();
  });
  return box as unknown as HTMLElement;
}

/** A cleared draft remounts the editor, so re-query instead of reusing the old node. */
async function clearedEditor(): Promise<void> {
  await waitFor(() => {
    const box = document.querySelector('textarea[aria-label="Write a comment"]');
    expect(box).not.toBeNull();
    expect(box).toHaveValue("");
  });
}

function submitButton(): HTMLElement {
  return screen.getByTestId("space-comment-composer-submit");
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.mocked(notifySpaceAttachmentFailure).mockClear();
  vi.mocked(notifySpaceCommentFailure).mockClear();
});

describe("SpaceCommentComposer", () => {
  it("posts on Enter and clears the draft (SPC-053)", async () => {
    const user = userEvent.setup();
    const fresh = comment({ id: "823e4567-e89b-12d3-a456-426614174008", comment_html: "<p>Hello there</p>" });
    const { calls } = renderComposer((call) =>
      call.method === "GET" ? { status: 200, body: [comment(), fresh] } : { status: 201, body: fresh }
    );
    const box = await editor();
    box.focus();
    expect(submitButton()).toBeDisabled();
    await user.keyboard("Hello there");
    await waitFor(() => expect(submitButton()).not.toBeDisabled());
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(true)
    );
    const post = calls.find((call) => call.method === "POST" && call.url.endsWith("/comments/"));
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ comment_html: "<p>Hello there</p>", access: "EXTERNAL" });
    expect(calls.some((call) => call.url.includes("/bulk/"))).toBe(false);
    await clearedEditor();
    expect(notifySpaceCommentFailure).not.toHaveBeenCalled();
  });

  it("posts through the send button too (SPC-053)", async () => {
    const user = userEvent.setup();
    const fresh = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { calls } = renderComposer(() => ({ status: 201, body: fresh }));
    const box = await editor();
    box.focus();
    await user.keyboard("Button post");
    await user.click(submitButton());
    await waitFor(() =>
      expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(true)
    );
    await clearedEditor();
  });

  it("newlines on Shift+Enter and leaves Ctrl/Cmd+Enter inert (SPC-053)", async () => {
    const user = userEvent.setup();
    const { calls } = renderComposer();
    const box = await editor();
    box.focus();
    await user.keyboard("one");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    await user.keyboard("{Control>}{Enter}{/Control}");
    await user.keyboard("{Meta>}{Enter}{/Meta}");
    await user.keyboard("two");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(false);
    expect(box).toHaveValue("one\ntwo");
  });

  it("ignores Enter on an empty draft (SPC-053)", async () => {
    const user = userEvent.setup();
    const { calls } = renderComposer();
    const box = await editor();
    box.focus();
    await user.keyboard("{Enter}");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(false);
  });

  it("leaves a composing Enter to the input (SPC-053)", async () => {
    const user = userEvent.setup();
    const { calls } = renderComposer();
    const box = await editor();
    box.focus();
    await user.keyboard("composing");
    await waitFor(() => expect(submitButton()).not.toBeDisabled());
    const composing = new KeyboardEvent("keydown", { key: "Enter", bubbles: true });
    Object.defineProperty(composing, "isComposing", { value: true });
    fireEvent(box, composing);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(false);
  });

  it("posts once while a write is out (SPC-053)", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const fresh = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { calls } = renderComposer(async (call) => {
      if (call.method === "POST" && call.url.endsWith("/comments/")) {
        await gate;
        return { status: 201, body: fresh };
      }
      return { status: 200, body: [] };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("Once only");
    await waitFor(() => expect(submitButton()).not.toBeDisabled());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(submitButton()).toBeDisabled());
    await user.keyboard("{Enter}");
    release();
    await clearedEditor();
    expect(calls.filter((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toHaveLength(1);
  });

  it("attaches an image and links it to the posted comment (SPC-053)", async () => {
    const user = userEvent.setup();
    const fresh = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { calls } = renderComposer((call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      if (call.method === "PATCH") return { status: 204, body: "" };
      if (call.url.includes("/bulk/")) return { status: 204, body: "" };
      if (call.method === "GET") return { status: 200, body: [] };
      return { status: 201, body: fresh };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("With a picture");
    const input = screen.getByTestId("space-comment-composer-image");
    await user.upload(input, new File(["bytes"], "pic.png", { type: "image/png" }));
    await waitFor(() => expect(screen.getByTestId("space-comment-composer").querySelector("img")).not.toBeNull());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(calls.some((call) => call.url.includes("/bulk/"))).toBe(true));
    const bulk = calls.find((call) => call.url.includes("/bulk/"));
    expect(bulk?.url).toContain(`/assets/v2/anchor/anchor-1/${fresh.id}/bulk/`);
    expect(JSON.parse(bulk?.body ?? "{}")).toEqual({ asset_ids: ["asset-1"] });
    expect(notifySpaceAttachmentFailure).not.toHaveBeenCalled();
    await clearedEditor();
  });

  it("drops only the failed upload and keeps the draft (SPC-053)", async () => {
    const user = userEvent.setup();
    const fresh = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { calls } = renderComposer((call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      if (call.method === "GET") return { status: 200, body: [] };
      return { status: 201, body: fresh };
    });
    vi.stubGlobal("fetch", async () => ({ ok: false, status: 500 }));
    const box = await editor();
    box.focus();
    await user.keyboard("Draft kept");
    const input = screen.getByTestId("space-comment-composer-image");
    await user.upload(input, new File(["bytes"], "pic.png", { type: "image/png" }));
    await waitFor(() => expect(notifySpaceAttachmentFailure).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("space-comment-composer").querySelector("img")).toBeNull();
    expect(box).toHaveValue("Draft kept");
    await user.keyboard("{Enter}");
    await clearedEditor();
    expect(calls.some((call) => call.url.includes("/bulk/"))).toBe(false);
  });

  it("ignores an empty file choice (SPC-053)", async () => {
    const { calls } = renderComposer();
    await editor();
    const input = screen.getByTestId("space-comment-composer-image");
    fireEvent.change(input, { target: { files: [] } });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls.some((call) => call.url.includes("/assets/v2/"))).toBe(false);
  });

  it("tolerates an unmount mid-upload (SPC-053)", async () => {
    const user = userEvent.setup();
    let release!: (value: { ok: boolean; status: number }) => void;
    const gate = new Promise<{ ok: boolean; status: number }>((resolve) => {
      release = resolve;
    });
    const { unmount } = renderComposer((call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      if (call.method === "PATCH") return { status: 204, body: "" };
      return { status: 200, body: [] };
    });
    vi.stubGlobal("fetch", () => gate);
    await editor();
    const input = screen.getByTestId("space-comment-composer-image");
    await user.upload(input, new File(["bytes"], "pic.png", { type: "image/png" }));
    unmount();
    release({ ok: true, status: 204 });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(notifySpaceAttachmentFailure).not.toHaveBeenCalled();
  });

  it("blocks submitting until the upload settles (SPC-053)", async () => {
    const user = userEvent.setup();
    let release!: (value: { ok: boolean; status: number }) => void;
    const gate = new Promise<{ ok: boolean; status: number }>((resolve) => {
      release = resolve;
    });
    const fresh = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { calls } = renderComposer((call) => {
      if (call.method === "POST" && call.url.endsWith("/assets/v2/anchor/anchor-1/")) {
        return { status: 201, body: GRANT };
      }
      if (call.method === "PATCH") return { status: 204, body: "" };
      if (call.url.includes("/bulk/")) return { status: 204, body: "" };
      if (call.method === "GET") return { status: 200, body: [] };
      return { status: 201, body: fresh };
    });
    vi.stubGlobal("fetch", () => gate);
    const box = await editor();
    box.focus();
    await user.keyboard("Hold the post");
    const input = screen.getByTestId("space-comment-composer-image");
    await user.upload(input, new File(["bytes"], "pic.png", { type: "image/png" }));
    await waitFor(() => expect(submitButton()).toBeDisabled());
    await user.keyboard("{Enter}");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(false);
    release({ ok: true, status: 204 });
    await waitFor(() => expect(submitButton()).not.toBeDisabled());
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(true)
    );
  });

  it("mentions a board member through the picker (SPC-054)", async () => {
    const user = userEvent.setup();
    const fresh = comment({ id: "823e4567-e89b-12d3-a456-426614174008" });
    const { calls } = renderComposer((call) => {
      if (call.url.endsWith("/members/")) return { status: 200, body: MEMBERS };
      if (call.method === "GET") return { status: 200, body: [] };
      return { status: 201, body: fresh };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("Hi @ad");
    await screen.findByRole("list", { name: "Mention suggestions" });
    await user.keyboard("{Enter}");
    await waitFor(() => expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument());
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toBe(true)
    );
    const post = calls.find((call) => call.method === "POST" && call.url.endsWith("/comments/"));
    expect(JSON.parse(post?.body ?? "{}").comment_html).toContain("mention-component");
    expect(JSON.parse(post?.body ?? "{}").comment_html).toContain('data-name="Ada L"');
  });

  it("dismisses the mention popup on Escape (SPC-054)", async () => {
    const user = userEvent.setup();
    renderComposer((call) => {
      if (call.url.endsWith("/members/")) return { status: 200, body: MEMBERS };
      return { status: 200, body: [] };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("Hi @ad");
    await screen.findByRole("list", { name: "Mention suggestions" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument());
    await user.keyboard("{Escape}");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
  });

  it("keeps the draft and the thread on a refused post (SPC-053)", async () => {
    const user = userEvent.setup();
    const { calls, queryClient } = renderComposer((call) => {
      if (call.method === "GET") return { status: 200, body: [comment()] };
      return { status: 500, body: { error: "boom" } };
    });
    queryClient.setQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID), [comment()]);
    const box = await editor();
    box.focus();
    await user.keyboard("Failed post");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(notifySpaceCommentFailure).toHaveBeenCalledTimes(1));
    expect(box).toHaveValue("Failed post");
    expect(queryClient.getQueryData(spaceKeys.comments(ANCHOR, ISSUE_ID))).toEqual([comment()]);
    expect(calls.filter((call) => call.method === "POST" && call.url.endsWith("/comments/"))).toHaveLength(1);
    expect(submitButton()).not.toBeDisabled();
  });
});
