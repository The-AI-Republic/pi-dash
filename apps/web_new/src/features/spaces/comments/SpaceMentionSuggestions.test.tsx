// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, type Mock, vi } from "vitest";

import type { RichTextEditorHandle } from "../../../shared/editor/index.js";
import { SpaceMentionSuggestions, useSpaceMentionPicker } from "./SpaceMentionSuggestions.js";

type PickerState = ReturnType<typeof useSpaceMentionPicker>;

function Harness({
  client,
  handle,
  busy = false,
  empty = false,
  probe,
}: {
  client: ReturnType<typeof createClient>;
  handle: RichTextEditorHandle | null;
  busy?: boolean;
  empty?: boolean;
  probe: { state?: PickerState };
}) {
  const picker = useSpaceMentionPicker({
    client,
    anchor: "anchor-1",
    getHandle: () => handle,
    busy,
    empty,
  });
  probe.state = picker;
  return (
    <div>{picker.mentionOpen ? <SpaceMentionSuggestions people={picker.people} onPick={picker.pick} /> : null}</div>
  );
}

function member(rowId: string, id: string, name: string, avatar: string) {
  return { id: rowId, member: id, member__display_name: name, member__avatar: avatar };
}

function fakeHandle(): RichTextEditorHandle & { insertMention: Mock<(target: { id: string; name: string }) => void> } {
  return {
    insertImage: () => undefined,
    insertMention: vi.fn(),
    insertHtml: () => undefined,
    textBeforeCaret: () => "",
  };
}

function renderPicker(
  props: Partial<React.ComponentProps<typeof Harness>> & { handle: RichTextEditorHandle | null },
  members: unknown[] = []
) {
  const calls: string[] = [];
  const transport: Transport = async (url: string) => {
    calls.push(url);
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(members),
    };
  };
  const client = createClient({ baseUrl: "https://api.test", transport });
  const probe: { state?: PickerState } = {};
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <Harness probe={probe} client={client} {...props} />
    </QueryClientProvider>
  );
  const state = (): PickerState => {
    if (probe.state === undefined) throw new Error("the picker published no state");
    return probe.state;
  };
  return { calls, state };
}

const ADA = "523e4567-e89b-12d3-a456-426614174004";
const GUEST = "623e4567-e89b-12d3-a456-426614174005";

describe("useSpaceMentionPicker", () => {
  it("stays closed without a query and opens with avatar rows (SPC-054)", async () => {
    const handle = fakeHandle();
    const { calls, state } = renderPicker({ handle }, [
      member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", "https://cdn.test/a.png"),
      member("823e4567-e89b-12d3-a456-426614174008", GUEST, "Guest G", ""),
    ]);
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
    expect(calls).toHaveLength(0);
    act(() => state().onMentionQuery(""));
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
    expect(calls).toHaveLength(0);
    act(() => state().onMentionQuery("g"));
    const popup = await screen.findByRole("list", { name: "Mention suggestions" });
    expect(within(popup).getByText("Users")).toBeInTheDocument();
    const rows = within(popup).getAllByRole("button");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("Guest G");
    expect(calls[0]).toContain("/api/public/anchor/anchor-1/members/");
  });

  it("shows stored avatars and initial fallbacks (SPC-054)", async () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle }, [
      member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", "https://cdn.test/a.png"),
    ]);
    act(() => state().onMentionQuery("ada"));
    const popup = await screen.findByRole("list", { name: "Mention suggestions" });
    const avatar = within(popup).getByRole("img", { name: "Ada L" });
    expect(avatar.querySelector("img")?.getAttribute("src")).toBe("https://cdn.test/a.png");
  });

  it("falls back to initials without a stored avatar (SPC-054)", async () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle }, [member("823e4567-e89b-12d3-a456-426614174008", GUEST, "Guest G", "")]);
    act(() => state().onMentionQuery("guest"));
    const popup = await screen.findByRole("list", { name: "Mention suggestions" });
    const avatar = within(popup).getByRole("img", { name: "Guest G" });
    expect(avatar.querySelector("img")).toBeNull();
    expect(avatar).toHaveTextContent("GG");
  });

  it("inserts the picked person at the caret and closes (SPC-054)", async () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle }, [member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", "")]);
    act(() => state().onMentionQuery("ada"));
    const popup = await screen.findByRole("list", { name: "Mention suggestions" });
    const row = within(popup).getByRole("button", { name: "Ada L Ada L" });
    fireEvent.mouseDown(row);
    expect(handle.insertMention).toHaveBeenCalledWith({ id: ADA, name: "Ada L" });
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
  });

  it("picks the first row on demand and tolerates an empty list (SPC-054)", async () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle }, [member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", "")]);
    act(() => state().onMentionQuery("ada"));
    await screen.findByRole("list", { name: "Mention suggestions" });
    act(() => state().pickFirst());
    expect(handle.insertMention).toHaveBeenCalledWith({ id: ADA, name: "Ada L" });
    const empty = renderPicker({ handle });
    act(() => empty.state().onMentionQuery("ada"));
    act(() => empty.state().pickFirst());
    expect(handle.insertMention).toHaveBeenCalledTimes(1);
  });

  it("tolerates a pick after the editor unmounted (SPC-054)", async () => {
    const { state } = renderPicker({ handle: null }, [
      member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", ""),
    ]);
    act(() => state().onMentionQuery("ada"));
    await screen.findByRole("list", { name: "Mention suggestions" });
    act(() => state().pickFirst());
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
  });

  it("keeps an Escape dismissal until the query changes (SPC-054)", async () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle }, [
      member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", ""),
      member("823e4567-e89b-12d3-a456-426614174008", GUEST, "Adam G", ""),
    ]);
    act(() => state().onMentionQuery("ad"));
    await screen.findByRole("list", { name: "Mention suggestions" });
    act(() => state().dismiss());
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
    act(() => state().onMentionQuery("ad"));
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
    act(() => state().onMentionQuery("ada"));
    await screen.findByRole("list", { name: "Mention suggestions" });
    act(() => state().reset());
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
  });

  it("closes with no match instead of offering strangers (SPC-054)", async () => {
    const handle = fakeHandle();
    const { calls, state } = renderPicker({ handle }, [
      member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", ""),
    ]);
    act(() => state().onMentionQuery("zzz"));
    await act(async () => {});
    expect(state().people).toEqual([]);
    expect(state().mentionOpen).toBe(false);
    expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument();
    expect(calls).toHaveLength(1);
  });

  it("swallows only the Enter the form must not see (SPC-053)", async () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle, busy: true }, [
      member("723e4567-e89b-12d3-a456-426614174006", ADA, "Ada L", ""),
    ]);
    const key = (init: KeyboardEventInit): KeyboardEvent => new KeyboardEvent("keydown", init);
    expect(state().handleEditorKeyDown(key({ key: "a" }))).toBe(false);
    expect(state().handleEditorKeyDown(key({ key: "Enter", shiftKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(key({ key: "Enter", ctrlKey: true }))).toBe(false);
    expect(state().handleEditorKeyDown(key({ key: "Enter", metaKey: true }))).toBe(false);
    const composing = key({ key: "Enter" });
    Object.defineProperty(composing, "isComposing", { value: true });
    expect(state().handleEditorKeyDown(composing)).toBe(false);
    // Busy with no open popup: the Enter is a no-op the surface swallows.
    expect(state().handleEditorKeyDown(key({ key: "Enter" }))).toBe(true);
    act(() => state().onMentionQuery("ada"));
    await screen.findByRole("list", { name: "Mention suggestions" });
    // An open popup swallows the Enter so the form can pick instead.
    expect(state().handleEditorKeyDown(key({ key: "Enter" }))).toBe(true);
  });

  it("lets a submit Enter reach the form (SPC-053)", () => {
    const handle = fakeHandle();
    const { state } = renderPicker({ handle, busy: false, empty: false });
    expect(state().handleEditorKeyDown(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(false);
  });
});
