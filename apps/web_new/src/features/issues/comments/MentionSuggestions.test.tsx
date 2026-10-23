// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { RichTextEditorHandle } from "../../../shared/editor/index.js";
import {
  CommentMentionSuggestions,
  MENTION_SUGGESTIONS_LABEL,
  MENTION_USERS_SECTION,
  useCommentMentionPicker,
} from "./MentionSuggestions.js";

type PickerState = ReturnType<typeof useCommentMentionPicker>;

function Harness({
  handle,
  busy = false,
  empty = false,
  projectId = "p-1",
  probe,
}: {
  handle: RichTextEditorHandle | null;
  busy?: boolean;
  empty?: boolean;
  projectId?: string;
  probe: { state?: PickerState };
}) {
  const picker = useCommentMentionPicker({
    workspaceSlug: "acme",
    projectId,
    getHandle: () => handle,
    busy,
    empty,
  });
  probe.state = picker;
  return (
    <div>{picker.mentionOpen ? <CommentMentionSuggestions people={picker.people} onPick={picker.pick} /> : null}</div>
  );
}

function person(id: string, name: string, avatar: string | null) {
  return { member__id: id, member__display_name: name, member__avatar_url: avatar };
}

function stubMentions(handler: (query: string) => unknown[]) {
  const calls: string[] = [];
  createAppClient("https://api.test", async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    calls.push(url);
    const query = new URL(url).searchParams.get("query") ?? "";
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify({ user_mention: handler(query) }),
    };
  });
  return calls;
}

function fakeHandle(): RichTextEditorHandle & { insertMention: ReturnType<typeof vi.fn> } {
  return {
    insertImage: () => undefined,
    insertMention: vi.fn(),
    insertHtml: () => undefined,
    textBeforeCaret: () => "",
  };
}

function renderPicker(
  props: Partial<React.ComponentProps<typeof Harness>> & { handle: RichTextEditorHandle | null },
  handler: (query: string) => unknown[] = () => []
) {
  const calls = stubMentions(handler);
  const probe: { state?: PickerState } = {};
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <Harness probe={probe} {...props} />
    </QueryClientProvider>
  );
  const state = (): PickerState => {
    if (probe.state === undefined) throw new Error("the picker published no state");
    return probe.state;
  };
  return { calls, state };
}

afterEach(() => {
  resetAppClient();
});

describe("useCommentMentionPicker", () => {
  it("stays closed without a query and opens with grouped avatar rows", async () => {
    const handle = fakeHandle();
    const { calls, state } = renderPicker({ handle }, () => [
      person("u-1", "Ada L", "https://cdn.test/a.png"),
      person("u-2", "Grace H", null),
    ]);
    expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument();
    expect(calls).toHaveLength(0);
    act(() => state().onMentionQuery("a"));
    const popup = await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    expect(within(popup).getByText(MENTION_USERS_SECTION)).toBeInTheDocument();
    const rows = within(popup).getAllByRole("button");
    expect(rows.map((row) => row.children[1]?.textContent)).toEqual(["Ada L", "Grace H"]);
    const avatars = within(popup).getAllByRole("img");
    expect(avatars.map((avatar) => avatar.getAttribute("aria-label"))).toEqual(["Ada L", "Grace H"]);
    expect(avatars[0]?.querySelector("img")?.getAttribute("src")).toBe("https://cdn.test/a.png");
    expect(avatars[1]?.querySelector("img")).toBeNull();
    expect(avatars[1]?.textContent).toBe("GH");
    expect(calls.some((url) => url.includes("query=a"))).toBe(true);
  });

  it("caps the popup at five rows", async () => {
    const { state } = renderPicker({ handle: fakeHandle() }, () =>
      Array.from({ length: 7 }, (_value, index) => person(`u-${index}`, `Member ${index}`, null))
    );
    act(() => state().onMentionQuery("m"));
    const popup = await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    expect(within(popup).getAllByRole("button")).toHaveLength(5);
  });

  it("narrows while typing and hides when nobody matches", async () => {
    const { calls, state } = renderPicker({ handle: fakeHandle() }, (query) =>
      query === "a" ? [person("u-1", "Ada L", null), person("u-2", "Grace H", null)] : []
    );
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    act(() => state().onMentionQuery("ax"));
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument()
    );
    expect(calls.some((url) => url.includes("query=ax"))).toBe(true);
  });

  it("picking a row inserts the mention and closes the popup", async () => {
    const user = userEvent.setup();
    const handle = fakeHandle();
    const { state } = renderPicker({ handle }, () => [person("u-1", "Ada L", null)]);
    act(() => state().onMentionQuery("a"));
    const popup = await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    await user.click(within(popup).getByRole("button"));
    expect(handle.insertMention).toHaveBeenCalledWith({ id: "u-1", name: "Ada L" });
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument()
    );
  });

  it("dismisses until the query changes, then reopens", async () => {
    const { state } = renderPicker({ handle: fakeHandle() }, () => [person("u-1", "Ada L", null)]);
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    act(() => state().dismiss());
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument()
    );
    // The same query re-reported keeps the dismissal in force.
    act(() => state().onMentionQuery("a"));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument();
    act(() => state().onMentionQuery("ad"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
  });

  it("reset forgets the query for a cleared draft", async () => {
    const { state } = renderPicker({ handle: fakeHandle() }, () => [person("u-1", "Ada L", null)]);
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    act(() => state().reset());
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument()
    );
    // The same text reported again opens fresh: no dismissal lingers.
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
  });

  it("pickFirst inserts the first row and tolerates a missing handle", async () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle }, () => [person("u-1", "Ada L", null)]);
    // Nothing to pick before the popup opens.
    act(() => state().pickFirst());
    expect(handle.insertMention).not.toHaveBeenCalled();
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    act(() => state().pickFirst());
    expect(handle.insertMention).toHaveBeenCalledWith({ id: "u-1", name: "Ada L" });
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument()
    );
  });

  it("closes without inserting when the editor is gone", async () => {
    const { state } = renderPicker({ handle: null }, () => [person("u-1", "Ada L", null)]);
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    act(() => state().pickFirst());
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: MENTION_SUGGESTIONS_LABEL })).not.toBeInTheDocument()
    );
  });

  it("swallows a plain Enter unless the form would submit (CMT-011)", async () => {
    const { state } = renderPicker({ handle: fakeHandle() }, () => [person("u-1", "Ada L", null)]);
    const enter = (overrides: KeyboardEventInit = {}) => new KeyboardEvent("keydown", { key: "Enter", ...overrides });
    // Closed and idle with content: the key bubbles on to submit.
    expect(state().handleEditorKeyDown(enter())).toBe(false);
    expect(state().handleEditorKeyDown(enter({ shiftKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(enter({ ctrlKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(enter({ metaKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "a" }))).toBe(false);
    const composing = enter();
    Object.defineProperty(composing, "isComposing", { value: true });
    expect(state().handleEditorKeyDown(composing)).toBe(false);
    // Open, busy or empty: swallowed, so no newline lands.
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    expect(state().handleEditorKeyDown(enter())).toBe(true);
  });

  it("swallows Enter while busy or empty with the popup closed", () => {
    const busy = renderPicker({ handle: fakeHandle(), busy: true }, () => []);
    expect(busy.state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(true);
    const empty = renderPicker({ handle: fakeHandle(), empty: true }, () => []);
    expect(empty.state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(true);
  });
});
