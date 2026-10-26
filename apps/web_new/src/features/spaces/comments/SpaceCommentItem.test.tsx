// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceComment, Transport, TransportInit } from "@pidash/api-client";
import { createClient } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown }));

vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));
vi.mock("../lib/spaceNotices.js", () => ({
  notifySpaceCommentDeleteFailure: vi.fn(),
  notifySpaceCommentDeleteSuccess: vi.fn(),
  notifySpaceCommentEditFailure: vi.fn(),
  notifySpaceCommentEditSuccess: vi.fn(),
  notifySpaceReactionFailure: vi.fn(),
}));

// The Tiptap chunk is lazy; the item's own behaviors are what this
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
    const boxRef = React.useRef<HTMLTextAreaElement>(null);
    const text = React.useRef(initialHtml.replace(/<[^>]*>/g, "").trim());
    const emit = (): void => {
      onChange(text.current.trim() === "" ? "<p></p>" : `<p>${text.current}</p>`);
      onMentionQuery?.(mentionQueryAt(text.current)?.text ?? null);
    };
    React.useEffect(
      () =>
        onReady?.({
          insertImage: () => undefined,
          insertMention: (target) => {
            const query = mentionQueryAt(text.current);
            if (query !== null) text.current = text.current.slice(0, text.current.length - query.text.length - 1);
            if (boxRef.current !== null) boxRef.current.value = text.current;
            text.current = `${text.current}[@${target.name}]`;
            if (boxRef.current !== null) boxRef.current.value = text.current;
            emit();
          },
        }),
      [onReady]
    );
    return (
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
    );
  }
  return { ...original, RichTextEditor: FakeEditor, CommentRichTextEditor: FakeEditor };
});

import { mentionQueryAt } from "../../../shared/editor/mention.js";
import { spaceKeys } from "../api/keys.js";
import {
  notifySpaceCommentDeleteFailure,
  notifySpaceCommentDeleteSuccess,
  notifySpaceCommentEditFailure,
  notifySpaceCommentEditSuccess,
  notifySpaceReactionFailure,
} from "../lib/spaceNotices.js";
import { SpaceCommentItem } from "./SpaceCommentItem.js";

const ANCHOR = "anchor-1";
const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const COMMENT_ID = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const GUEST = "623e4567-e89b-12d3-a456-426614174005";

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

function viewerPayload() {
  return {
    id: ADA,
    email: "ada@test.dev",
    username: "ada",
    first_name: "Ada",
    last_name: "L",
    display_name: "Ada L",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2026-01-01T00:00:00Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "password",
    last_login_time: null,
  };
}

type HandlerAnswer = { status: number; body: unknown };

function renderItem(
  props: Partial<React.ComponentProps<typeof SpaceCommentItem>> = {},
  handler: (call: { method: string; url: string; body: string }) => HandlerAnswer | Promise<HandlerAnswer> = () => ({
    status: 200,
    body: [],
  })
) {
  const calls: { method: string; url: string; body: string }[] = [];
  const viewerId = props.viewerId === undefined ? ADA : props.viewerId;
  const transport: Transport = async (url: string, init?: TransportInit) => {
    const call = { method: init?.method ?? "GET", url, body: init?.body ?? "" };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    if (url.endsWith("/api/users/me/")) {
      if (viewerId === null) {
        return { status: 401, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "anon" }) };
      }
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(viewerPayload()),
      };
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
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const onRequireSignIn = props.onRequireSignIn ?? vi.fn();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SpaceCommentItem
        anchor={ANCHOR}
        issueId={ISSUE_ID}
        comment={comment()}
        viewerId={ADA}
        embedded={false}
        reactionsEnabled
        onRequireSignIn={onRequireSignIn}
        resolveAsset={(src) => src}
        {...props}
      />
    </QueryClientProvider>
  );
  return { calls, onRequireSignIn, queryClient, unmount: view.unmount };
}

function seedViewer(queryClient: QueryClient): void {
  queryClient.setQueryData([...spaceKeys.all, "viewer"], viewerPayload());
}

async function openMenu(): Promise<void> {
  const user = userEvent.setup();
  await user.click(screen.getByTestId("space-comment-menu"));
  await screen.findByRole("menu");
}

afterEach(() => {
  vi.mocked(notifySpaceCommentDeleteFailure).mockClear();
  vi.mocked(notifySpaceCommentDeleteSuccess).mockClear();
  vi.mocked(notifySpaceCommentEditFailure).mockClear();
  vi.mocked(notifySpaceCommentEditSuccess).mockClear();
  vi.mocked(notifySpaceReactionFailure).mockClear();
});

describe("SpaceCommentItem", () => {
  it("offers exactly Edit and Delete on the author's own row (SPC-055)", async () => {
    renderItem();
    await openMenu();
    const menu = screen.getByRole("menu");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((entry) => entry.textContent)
    ).toEqual(["Edit", "Delete"]);
  });

  it("shows no menu on other visitors' rows (SPC-055)", () => {
    renderItem({ viewerId: GUEST });
    expect(screen.getByText("Ada L")).toBeInTheDocument();
    expect(screen.queryByTestId("space-comment-menu")).not.toBeInTheDocument();
  });

  it("shows no menu to anonymous visitors (SPC-055)", () => {
    renderItem({ viewerId: null });
    expect(screen.queryByTestId("space-comment-menu")).not.toBeInTheDocument();
  });

  it("shows no menu inside embeds (SPC-055)", () => {
    renderItem({ embedded: true });
    expect(screen.getByText("Ada L")).toBeInTheDocument();
    expect(screen.queryByTestId("space-comment-menu")).not.toBeInTheDocument();
  });

  it("edits inline and swaps the saved body at once (SPC-056)", async () => {
    const user = userEvent.setup();
    const stored = comment({ comment_html: "<p>Hello edited</p>" });
    const { calls } = renderItem({}, (call) => {
      if (call.method === "GET" && call.url.includes("/comments/")) return { status: 200, body: [stored] };
      if (call.method === "PATCH") return { status: 200, body: stored };
      return { status: 200, body: [] };
    });
    await openMenu();
    await user.click(screen.getByRole("menuitem", { name: "Edit" }));
    const box = await screen.findByRole("textbox", { name: "Edit comment" });
    expect(box).toHaveValue("Hello");
    box.focus();
    await user.keyboard("{End} edited");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(screen.queryByTestId("space-comment-edit")).not.toBeInTheDocument());
    const patch = calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ comment_html: "<p>Hello edited</p>" });
    expect(notifySpaceCommentEditSuccess).toHaveBeenCalledTimes(1);
    expect(notifySpaceCommentEditFailure).not.toHaveBeenCalled();
  });

  it("reconciles a refused save to the stored text (SPC-056)", async () => {
    const user = userEvent.setup();
    renderItem({}, () => ({ status: 500, body: { error: "boom" } }));
    await openMenu();
    await user.click(screen.getByRole("menuitem", { name: "Edit" }));
    const box = await screen.findByRole("textbox", { name: "Edit comment" });
    box.focus();
    await user.keyboard("{End} edited");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(notifySpaceCommentEditFailure).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByTestId("space-comment-edit")).not.toBeInTheDocument());
    expect(screen.getByText("Hello")).toBeInTheDocument();
  });

  it("cancels an edit without a write (SPC-056)", async () => {
    const user = userEvent.setup();
    const { calls } = renderItem();
    await openMenu();
    await user.click(screen.getByRole("menuitem", { name: "Edit" }));
    await screen.findByRole("textbox", { name: "Edit comment" });
    await user.click(screen.getByRole("button", { name: "Cancel editing" }));
    await waitFor(() => expect(screen.queryByTestId("space-comment-edit")).not.toBeInTheDocument());
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(notifySpaceCommentEditSuccess).not.toHaveBeenCalled();
    expect(notifySpaceCommentEditFailure).not.toHaveBeenCalled();
  });

  it("deletes immediately with a success notice (SPC-055)", async () => {
    const user = userEvent.setup();
    const { calls } = renderItem({}, (call) => {
      if (call.method === "DELETE") return { status: 204, body: "" };
      return { status: 200, body: [] };
    });
    await openMenu();
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await waitFor(() =>
      expect(calls.some((call) => call.method === "DELETE" && call.url.includes(`/comments/${COMMENT_ID}/`))).toBe(true)
    );
    expect(notifySpaceCommentDeleteSuccess).toHaveBeenCalledTimes(1);
    expect(notifySpaceCommentDeleteFailure).not.toHaveBeenCalled();
  });

  it("restores a refused delete with an error notice (SPC-055)", async () => {
    const user = userEvent.setup();
    renderItem({}, () => ({ status: 500, body: { error: "boom" } }));
    await openMenu();
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await waitFor(() => expect(notifySpaceCommentDeleteFailure).toHaveBeenCalledTimes(1));
    expect(notifySpaceCommentDeleteSuccess).not.toHaveBeenCalled();
    expect(screen.getByText("Hello")).toBeInTheDocument();
  });

  it("toggles comment reactions through pills and the picker (SPC-057)", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { calls, queryClient } = renderItem({ comment: comment({ comment_reactions: [] }) }, async (call) => {
      if (call.method === "POST" && call.url.includes("/reactions/")) {
        await gate;
        return {
          status: 201,
          body: {
            id: "723e4567-e89b-12d3-a456-426614174006",
            actor: ADA,
            comment: COMMENT_ID,
            reaction: "127881",
            display_name: "Ada L",
            deleted_at: null,
            workspace: "423e4567-e89b-12d3-a456-426614174003",
            project: "323e4567-e89b-12d3-a456-426614174002",
            created_at: "2026-10-16T22:00:00Z",
            updated_at: "2026-10-16T22:00:00Z",
            created_by: null,
            updated_by: null,
          },
        };
      }
      return { status: 200, body: [] };
    });
    seedViewer(queryClient);
    expect(screen.getByTestId("space-comment-reactions")).toBeInTheDocument();
    await user.click(screen.getByTestId("space-comment-reaction-add"));
    await user.click(screen.getByRole("button", { name: "🎉" }));
    await waitFor(() =>
      expect(
        calls.some((call) => call.method === "POST" && call.url.includes(`/comments/${COMMENT_ID}/reactions/`))
      ).toBe(true)
    );
    // The pending toggle paints its pill before the write lands.
    const pill = await screen.findByTestId("space-comment-reaction");
    expect(pill).toHaveTextContent("1");
    release();
    // Settling reconciles to the server rows (still empty here).
    await waitFor(() => expect(screen.queryByTestId("space-comment-reaction")).not.toBeInTheDocument());
    expect(notifySpaceReactionFailure).not.toHaveBeenCalled();
  });

  it("hides the reaction row while the publisher flag is off (SPC-057)", () => {
    renderItem({ reactionsEnabled: false });
    expect(screen.queryByTestId("space-comment-reactions")).not.toBeInTheDocument();
  });

  it("sends anonymous reaction attempts to sign-in (SPC-057)", async () => {
    const user = userEvent.setup();
    const onRequireSignIn = vi.fn();
    const { calls } = renderItem({ viewerId: null, onRequireSignIn });
    await user.click(screen.getByTestId("space-comment-reaction-add"));
    await user.click(screen.getByRole("button", { name: "🎉" }));
    await waitFor(() => expect(onRequireSignIn).toHaveBeenCalledTimes(1));
    expect(calls.some((call) => call.url.includes("/reactions/"))).toBe(false);
  });

  it("shows static reaction counts inside embeds (SPC-057)", () => {
    renderItem({
      embedded: true,
      comment: comment({
        comment_reactions: [
          {
            id: "723e4567-e89b-12d3-a456-426614174006",
            actor: GUEST,
            comment: COMMENT_ID,
            reaction: "127881",
            display_name: "Guest",
            deleted_at: null,
            workspace: "423e4567-e89b-12d3-a456-426614174003",
            project: "323e4567-e89b-12d3-a456-426614174002",
            created_at: "2026-10-16T22:00:00Z",
            updated_at: "2026-10-16T22:00:00Z",
            created_by: null,
            updated_by: null,
          },
        ],
      }),
    });
    const pill = screen.getByTestId("space-comment-reaction");
    expect(pill.tagName).toBe("SPAN");
    expect(pill).toHaveTextContent("1");
    expect(screen.queryByTestId("space-comment-reaction-add")).not.toBeInTheDocument();
  });

  it("reports a refused reaction toggle (SPC-057)", async () => {
    const user = userEvent.setup();
    const { queryClient } = renderItem({ comment: comment({ comment_reactions: [] }) }, (call) => {
      if (call.method === "POST" && call.url.includes("/reactions/")) {
        return { status: 500, body: { error: "boom" } };
      }
      return { status: 200, body: [] };
    });
    seedViewer(queryClient);
    await user.click(screen.getByTestId("space-comment-reaction-add"));
    await user.click(screen.getByRole("button", { name: "🎉" }));
    await waitFor(() => expect(notifySpaceReactionFailure).toHaveBeenCalledTimes(1));
    expect(screen.queryByTestId("space-comment-reaction")).not.toBeInTheDocument();
  });
});
