// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";

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
  submit = () => undefined,
}: {
  handle: RichTextEditorHandle | null;
  busy?: boolean;
  empty?: boolean;
  projectId?: string;
  probe: { state?: PickerState };
  submit?: (liveHtml: string | null) => void;
}) {
  const picker = useCommentMentionPicker({
    workspaceSlug: "acme",
    projectId,
    getHandle: () => handle,
    busy,
    empty,
    submit,
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

function fakeHandle(
  liveHtml = "<p>Draft</p>"
): RichTextEditorHandle & { insertMention: Mock<(target: { id: string; name: string }) => void> } {
  return {
    insertImage: () => undefined,
    insertMention: vi.fn(),
    insertHtml: () => undefined,
    textBeforeCaret: () => "",
    getHtml: () => liveHtml,
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

  it("owns a plain Enter and submits the live draft (CMT-011, ISS-197)", () => {
    const submit = vi.fn();
    const { state } = renderPicker({ handle: fakeHandle("<p>Fresh words</p>"), empty: false, submit }, () => []);
    expect(state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(true);
    expect(submit).toHaveBeenCalledTimes(1);
    expect(submit).toHaveBeenCalledWith("<p>Fresh words</p>");
  });

  it("reads the live document when the snapshot is still empty (ISS-197)", () => {
    // The keystroke filled the editor, but React has not rendered it.
    const submit = vi.fn();
    const { state } = renderPicker({ handle: fakeHandle("<p>Just typed</p>"), empty: true, submit }, () => []);
    expect(state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(true);
    expect(submit).toHaveBeenCalledWith("<p>Just typed</p>");
  });

  it("lets every other Enter shape through untouched (CMT-011)", () => {
    const submit = vi.fn();
    const { state } = renderPicker({ handle: fakeHandle(), empty: false, submit }, () => []);
    const enter = (overrides: KeyboardEventInit = {}) => new KeyboardEvent("keydown", { key: "Enter", ...overrides });
    expect(state().handleEditorKeyDown(enter({ shiftKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(enter({ ctrlKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(enter({ metaKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "a" }))).toBe(false);
    const composing = enter();
    Object.defineProperty(composing, "isComposing", { value: true });
    expect(state().handleEditorKeyDown(composing)).toBe(false);
    expect(submit).not.toHaveBeenCalled();
  });

  it("picks the open mention instead of submitting (CMT-019)", async () => {
    const submit = vi.fn();
    const handle = fakeHandle();
    const { state } = renderPicker({ handle, empty: false, submit }, () => [person("u-1", "Ada L", null)]);
    act(() => state().onMentionQuery("a"));
    await screen.findByRole("list", { name: MENTION_SUGGESTIONS_LABEL });
    expect(state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(true);
    expect(handle.insertMention).toHaveBeenCalledWith({ id: "u-1", name: "Ada L" });
    expect(submit).not.toHaveBeenCalled();
  });

  it("holds Enter while busy or empty with the popup closed (CMT-011)", () => {
    const enter = new KeyboardEvent("keydown", { key: "Enter" });
    const busySubmit = vi.fn();
    const busy = renderPicker({ handle: fakeHandle("<p>Held</p>"), busy: true, submit: busySubmit }, () => []);
    expect(busy.state().handleEditorKeyDown(enter)).toBe(true);
    expect(busySubmit).not.toHaveBeenCalled();
    const emptySubmit = vi.fn();
    const empty = renderPicker({ handle: fakeHandle("<p></p>"), empty: true, submit: emptySubmit }, () => []);
    expect(empty.state().handleEditorKeyDown(enter)).toBe(true);
    expect(emptySubmit).not.toHaveBeenCalled();
  });

  it("holds Enter when the editing surface is gone and the snapshot is empty", () => {
    const submit = vi.fn();
    const { state } = renderPicker({ handle: null, empty: true, submit }, () => []);
    expect(state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(true);
    expect(submit).not.toHaveBeenCalled();
  });
});
