// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page assistant menu (parity/specs/pages.md §9.3, PAGE-075): the gutter
// entry, the instruction plus tone task run, the read-only reply with
// apply/append/regenerate, and the inline loading, failure and empty
// states.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { jsonResponse } from "../testing.js";
import {
  PAGE_ASSISTANT_APPEND_LABEL,
  PAGE_ASSISTANT_APPLY_LABEL,
  PAGE_ASSISTANT_CLOSE_LABEL,
  PAGE_ASSISTANT_EMPTY_INSTRUCTION,
  PAGE_ASSISTANT_GENERATE_LABEL,
  PAGE_ASSISTANT_GENERATING_LABEL,
  PAGE_ASSISTANT_HANDLE_LABEL,
  PAGE_ASSISTANT_INSTRUCTION_LABEL,
  PAGE_ASSISTANT_LOADING_LABEL,
  PAGE_ASSISTANT_PANEL_LABEL,
  PAGE_ASSISTANT_REGENERATE_LABEL,
} from "../lib/pageAssistant.js";
import { PAGE_ASSISTANT_EMPTY_REPLY, PageAssistantHandle, PageAssistantMenu } from "./PageAssistantMenu.js";

let posts: { url: string; body: unknown }[] = [];
let answer: { response: string; response_html: string } = { response: "done", response_html: "<p>done</p>" };
let status = 200;
let holdPosts = false;
let held: Array<() => void> = [];

beforeEach(() => {
  posts = [];
  answer = { response: "done", response_html: "<p>done</p>" };
  status = 200;
  holdPosts = false;
  held = [];
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (init.method === "POST") {
      posts.push({ url, body: init.body === undefined ? null : JSON.parse(init.body) });
      if (holdPosts) await new Promise<void>((resolve) => held.push(resolve));
      if (status !== 200) return jsonResponse(url, status, { error: "Task is required" });
      return jsonResponse(url, status, answer);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

function releasePosts(): void {
  held.forEach((resolve) => resolve());
  held = [];
}

afterEach(() => {
  resetAppClient();
});

function harness(ui: React.ReactElement): void {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}

function menuProps(overrides: Partial<React.ComponentProps<typeof PageAssistantMenu>> = {}) {
  return {
    workspaceSlug: "acme",
    selection: "chosen text",
    onApply: vi.fn(),
    onAppend: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
}

describe("PageAssistantHandle", () => {
  it("announces the Ask entry and asks on click", async () => {
    const user = userEvent.setup();
    const onAsk = vi.fn();
    render(<PageAssistantHandle onAsk={onAsk} />);
    const entry = screen.getByTestId("page-ai-handle");
    expect(entry).toHaveAccessibleName(PAGE_ASSISTANT_HANDLE_LABEL);
    expect(PAGE_ASSISTANT_HANDLE_LABEL).toMatch(/^Ask Pi Dash\b/);
    await user.click(entry);
    expect(onAsk).toHaveBeenCalledTimes(1);
  });
});

describe("PageAssistantMenu", () => {
  it("asks for an instruction before sending anything", async () => {
    const user = userEvent.setup();
    harness(<PageAssistantMenu {...menuProps()} />);
    expect(screen.getByRole("dialog", { name: PAGE_ASSISTANT_PANEL_LABEL })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    expect(await screen.findByText(PAGE_ASSISTANT_EMPTY_INSTRUCTION)).toBeInTheDocument();
    expect(posts).toEqual([]);
  });

  it("runs the instruction over the selection and shows the reply read-only", async () => {
    const user = userEvent.setup();
    holdPosts = true;
    harness(<PageAssistantMenu {...menuProps()} />);
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    expect(await screen.findByText(PAGE_ASSISTANT_LOADING_LABEL)).toBeInTheDocument();
    releasePosts();
    const reply = await screen.findByTestId("page-assistant-reply");
    expect(reply).toHaveTextContent("done");
    expect(reply).not.toHaveAttribute("contenteditable");
    expect(posts).toEqual([
      {
        url: "https://api.test/api/workspaces/acme/ai-assistant/",
        body: { task: "Summarize", prompt: "chosen text" },
      },
    ]);
    expect(screen.getByRole("button", { name: PAGE_ASSISTANT_REGENERATE_LABEL })).toBeInTheDocument();
  });

  it("cleans the reply markup before showing it", async () => {
    const user = userEvent.setup();
    answer = { response: "done", response_html: "<p>done</p><script>evil()</script>" };
    harness(<PageAssistantMenu {...menuProps()} />);
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    const reply = await screen.findByTestId("page-assistant-reply");
    expect(reply.innerHTML).not.toContain("script");
  });

  it("submits on Enter but keeps Shift+Enter for newlines", async () => {
    const user = userEvent.setup();
    harness(<PageAssistantMenu {...menuProps()} />);
    const field = screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL);
    await user.type(field, "Summarize");
    fireEvent.keyDown(field, { key: "Enter", shiftKey: true });
    expect(posts).toEqual([]);
    fireEvent.keyDown(field, { key: "Enter", shiftKey: false });
    await screen.findByTestId("page-assistant-reply");
    expect(posts).toHaveLength(1);
  });

  it("sends the tone direction with the instruction", async () => {
    const user = userEvent.setup();
    harness(<PageAssistantMenu {...menuProps()} />);
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("radio", { name: "Formal" }));
    // Arming a preset before the first run sends nothing.
    expect(posts).toEqual([]);
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    await screen.findByTestId("page-assistant-reply");
    expect(posts[0]?.body).toEqual({
      task: "Summarize\n\nAnswer in a formal tone.",
      prompt: "chosen text",
    });
    // A preset picked after a reply re-runs the task with the new tone.
    await user.click(screen.getByRole("radio", { name: "Casual" }));
    await waitFor(() => expect(posts).toHaveLength(2));
    expect(posts[1]?.body).toEqual({
      task: "Summarize\n\nAnswer in a casual tone.",
      prompt: "chosen text",
    });
  });

  it("applies the reply in place or appends it below", async () => {
    const user = userEvent.setup();
    const props = menuProps();
    harness(<PageAssistantMenu {...props} />);
    expect(screen.queryByRole("button", { name: PAGE_ASSISTANT_APPLY_LABEL })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: PAGE_ASSISTANT_APPEND_LABEL })).not.toBeInTheDocument();
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    await screen.findByTestId("page-assistant-reply");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_APPLY_LABEL }));
    expect(props.onApply).toHaveBeenCalledWith("<p>done</p>");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_APPEND_LABEL }));
    expect(props.onAppend).toHaveBeenCalledWith("<p>done</p>");
  });

  it("regenerates the same task on demand", async () => {
    const user = userEvent.setup();
    harness(<PageAssistantMenu {...menuProps()} />);
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    await screen.findByTestId("page-assistant-reply");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_REGENERATE_LABEL }));
    await waitFor(() => expect(posts).toHaveLength(2));
    expect(posts[0]?.body).toEqual(posts[1]?.body);
  });

  it("disables the submit while a task is in flight", async () => {
    const user = userEvent.setup();
    holdPosts = true;
    harness(<PageAssistantMenu {...menuProps()} />);
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    expect(await screen.findByRole("button", { name: PAGE_ASSISTANT_GENERATING_LABEL })).toBeDisabled();
    releasePosts();
    await screen.findByTestId("page-assistant-reply");
  });

  it("renders failures inline", async () => {
    const user = userEvent.setup();
    status = 400;
    harness(<PageAssistantMenu {...menuProps()} />);
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    expect(await screen.findByTestId("page-assistant-error")).toHaveTextContent("Task is required");
    expect(screen.queryByTestId("page-assistant-reply")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: PAGE_ASSISTANT_APPLY_LABEL })).not.toBeInTheDocument();
  });

  it("reports an empty reply without offering apply controls", async () => {
    const user = userEvent.setup();
    answer = { response: "", response_html: "" };
    harness(<PageAssistantMenu {...menuProps()} />);
    await user.type(screen.getByLabelText(PAGE_ASSISTANT_INSTRUCTION_LABEL), "Summarize");
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_GENERATE_LABEL }));
    expect(await screen.findByText(PAGE_ASSISTANT_EMPTY_REPLY)).toBeInTheDocument();
    expect(screen.queryByTestId("page-assistant-reply")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: PAGE_ASSISTANT_APPLY_LABEL })).not.toBeInTheDocument();
  });

  it("closes on request", async () => {
    const user = userEvent.setup();
    const props = menuProps();
    harness(<PageAssistantMenu {...props} />);
    await user.click(screen.getByRole("button", { name: PAGE_ASSISTANT_CLOSE_LABEL }));
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });
});
