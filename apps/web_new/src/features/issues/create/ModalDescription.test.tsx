// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modal description panel (parity/specs/issues.md §11, ISS-125/127). The
// panel composes the editor with the assistant footer, the person lookup
// and the image upload; the editor chunk itself has its own suite, so a
// stand-in exposes the same handle surface and inspects what the panel
// asks of it.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { RichTextEditorHandle } from "../../../shared/editor/index.js";
import {
  AI_BUTTON_LABEL,
  DESCRIBE_BUTTON_LABEL,
  DESCRIBE_BUSY_LABEL,
  ModalDescription,
  hasDescriptionContent,
  type ModalDescriptionProps,
} from "./ModalDescription.js";
import { DESCRIBE_FAILED_TITLE, DESCRIBE_RATE_LIMITED_BODY, DESCRIBE_TASK } from "./createApi.js";

vi.setConfig({ testTimeout: 30_000 });

interface EditorProbe {
  onChange: (html: string) => void;
  tabIndex?: number | undefined;
  handleKeyDown?: ((event: KeyboardEvent) => boolean) | undefined;
  onMentionQuery?: ((query: string | null) => void) | undefined;
  onReady?: ((handle: RichTextEditorHandle) => void) | undefined;
  initialHtml: string;
}

const probes: EditorProbe[] = [];

vi.mock("../../../shared/editor/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/index.js")>();
  function StandInEditor(props: EditorProbe) {
    probes.push(props);
    React.useEffect(() => {
      props.onReady?.({
        insertImage: vi.fn(),
        insertMention: vi.fn(),
        textBeforeCaret: () => "",
      });
    }, [props]);
    return (
      <textarea
        aria-label="Description"
        defaultValue={props.initialHtml.replace(/^<p>/, "").replace(/<\/p>$/, "")}
        onChange={(event) => props.onChange(event.target.value === "" ? "" : `<p>${event.target.value}</p>`)}
        onKeyDown={(event) => {
          if (props.handleKeyDown?.(event.nativeEvent) === true) event.preventDefault();
        }}
      />
    );
  }
  return { ...original, IssueDescriptionEditor: StandInEditor };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const SLUG = "acme";
const PROJECT = "11111111-1111-4111-8111-111111111111";

/** One issue-modal editor answer with the details the contract echoes. */
function editorAnswer(responseHtml: string) {
  return {
    response: responseHtml,
    response_html: responseHtml,
    project_detail: {
      id: PROJECT,
      identifier: "ATL",
      name: "Atlas",
      cover_image: null,
      cover_image_url: null,
      logo_props: {},
      description: "",
      is_default: true,
    },
    workspace_detail: { name: "Acme", slug: SLUG, id: PROJECT, logo_url: null },
  };
}

interface StubOptions {
  describeStatus?: number;
  describeBody?: unknown;
  /** Hold the editor answer so the pending state is observable. */
  describeDelayMs?: number;
  answerStatus?: number;
  uploadStatus?: number;
  mentions?: Array<{ member__id: string; member__display_name: string; member__avatar_url: string | null }>;
}

function stubTransport(options: StubOptions = {}) {
  const calls: Array<{ method: string; url: string; body: unknown }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    const json = (status: number, payload: unknown) => ({
      status,
      ok: status < 400,
      url: "https://api.test",
      headers: new Headers({ "content-type": "application/json" }),
      text: async () => JSON.stringify(payload),
    });
    if (url.includes("/entity-search/")) {
      return json(200, { user_mention: options.mentions ?? [] });
    }
    if (url.includes("/ai-assistant/")) {
      if (options.describeDelayMs !== undefined) {
        await new Promise((resolve) => globalThis.setTimeout(resolve, options.describeDelayMs));
      }
      const status = options.answerStatus ?? options.describeStatus ?? 200;
      return json(status, status < 400 ? (options.describeBody ?? editorAnswer("<p>assistant words</p>")) : {});
    }
    if (url.includes("/api/assets/v2/") && method === "POST") {
      const status = options.uploadStatus ?? 200;
      return json(status, {
        asset_id: "asset-1",
        asset_url: "https://cdn.test/asset-1.png",
        upload_data: { url: "https://s3.test/upload", fields: { key: "k" } },
      });
    }
    return json(204, {});
  }) as never);
  return { calls };
}

function setup(options: StubOptions = {}, props: Partial<ModalDescriptionProps> = {}) {
  const stub = stubTransport(options);
  const onChange = vi.fn();
  const onAssetUploaded = vi.fn();
  const onSubmit = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <ModalDescription
        workspaceSlug={SLUG}
        projectId={PROJECT}
        value="<p>start</p>"
        onChange={onChange}
        disabled={false}
        title="A title"
        assistantConfigured
        assetScope={{ entityType: "ISSUE_DESCRIPTION", entityId: "" }}
        onAssetUploaded={onAssetUploaded}
        generation={0}
        onSubmit={onSubmit}
        {...props}
      />
    </QueryClientProvider>
  );
  return { ...stub, onChange, onAssetUploaded, onSubmit, queryClient, view };
}

function lastProbe(): EditorProbe {
  return probes[probes.length - 1]!;
}

beforeEach(() => {
  probes.length = 0;
  vi.clearAllMocks();
  globalThis.fetch = vi.fn(async () => new Response(null, { status: 204 })) as never;
});

afterEach(() => {
  resetAppClient();
});

describe("hasDescriptionContent", () => {
  it("counts words, images and mentions as content", () => {
    expect(hasDescriptionContent("<p>words</p>")).toBe(true);
    expect(hasDescriptionContent('<p><img src="x"></p>')).toBe(true);
    expect(hasDescriptionContent('<p><mention-component id="u"></mention-component></p>')).toBe(true);
    expect(hasDescriptionContent("<p></p>")).toBe(false);
  });
});

describe("ModalDescription assistant (ISS-127)", () => {
  it("drafts a body from the title and adopts it in the editor", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: DESCRIBE_BUTTON_LABEL }));
    await waitFor(() => expect(h.onChange).toHaveBeenCalledWith("<p>assistant words</p>"));
    const call = h.calls.find((entry) => entry.url.includes("/ai-assistant/"));
    expect(call?.body).toEqual({ task: DESCRIBE_TASK, prompt: "A title" });
    expect(screen.getByRole("button", { name: DESCRIBE_BUTTON_LABEL })).toBeInTheDocument();
  });

  it("shows the busy label while drafting", async () => {
    const user = userEvent.setup();
    setup({ describeDelayMs: 200 });
    await user.click(screen.getByRole("button", { name: DESCRIBE_BUTTON_LABEL }));
    expect(await screen.findByRole("button", { name: DESCRIBE_BUSY_LABEL })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: DESCRIBE_BUTTON_LABEL })).toBeInTheDocument();
  });

  it("refuses to draft from an empty title", async () => {
    const h = setup({}, { title: "   " });
    expect(screen.queryByRole("button", { name: DESCRIBE_BUTTON_LABEL })).not.toBeInTheDocument();
    expect(h.calls.some((entry) => entry.url.includes("/ai-assistant/"))).toBe(false);
  });

  it("reports an empty answer as a failure", async () => {
    const user = userEvent.setup();
    const h = setup({ describeBody: editorAnswer("   ") });
    await user.click(screen.getByRole("button", { name: DESCRIBE_BUTTON_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(DESCRIBE_FAILED_TITLE, expect.any(String)));
    expect(h.onChange).not.toHaveBeenCalled();
  });

  it("reads a 429 as the monthly limit", async () => {
    const user = userEvent.setup();
    setup({ describeStatus: 429 });
    await user.click(screen.getByRole("button", { name: DESCRIBE_BUTTON_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(DESCRIBE_FAILED_TITLE, DESCRIBE_RATE_LIMITED_BODY));
  });

  it("shows other failures as a retry notice", async () => {
    const user = userEvent.setup();
    setup({ describeStatus: 500 });
    await user.click(screen.getByRole("button", { name: DESCRIBE_BUTTON_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(DESCRIBE_FAILED_TITLE, expect.any(String)));
    expect(notifyError).not.toHaveBeenCalledWith(DESCRIBE_FAILED_TITLE, DESCRIBE_RATE_LIMITED_BODY);
  });

  it("hides the assistant entries when no model is configured", async () => {
    setup({}, { assistantConfigured: false });
    expect(screen.queryByRole("button", { name: DESCRIBE_BUTTON_LABEL })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: AI_BUTTON_LABEL })).not.toBeInTheDocument();
  });

  it("inserts a GPT answer at the caret", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: AI_BUTTON_LABEL }));
    const prompt = await screen.findByLabelText("Ask the assistant");
    await user.type(prompt, "Make it shorter");
    await user.click(screen.getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(h.onChange).toHaveBeenCalledWith("<p>start</p><p>assistant words</p>"));
    expect(notifySuccess).toHaveBeenCalledWith("Inserted", expect.any(String));
    const call = h.calls.find((entry) => entry.url.includes("/ai-assistant/"));
    expect(call?.body).toEqual({ task: "Make it shorter" });
  });

  it("inserts a bare answer into an empty body", async () => {
    const user = userEvent.setup();
    const h = setup({}, { value: "" });
    await user.click(screen.getByRole("button", { name: AI_BUTTON_LABEL }));
    await user.type(await screen.findByLabelText("Ask the assistant"), "go");
    await user.click(screen.getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(h.onChange).toHaveBeenCalledWith("<p>assistant words</p>"));
  });

  it("keeps Insert disabled without a prompt and on an empty answer", async () => {
    const user = userEvent.setup();
    const h = setup({}, {});
    await user.click(screen.getByRole("button", { name: AI_BUTTON_LABEL }));
    const insert = await screen.findByRole("button", { name: "Insert" });
    expect(insert).toBeDisabled();
    await user.type(screen.getByLabelText("Ask the assistant"), "prompt");
    expect(insert).toBeEnabled();
    expect(h.calls.some((entry) => entry.url.includes("/ai-assistant/"))).toBe(false);
  });

  it("reports a failed GPT answer without changing the body", async () => {
    const user = userEvent.setup();
    const h = setup({ answerStatus: 500 });
    await user.click(screen.getByRole("button", { name: AI_BUTTON_LABEL }));
    await user.type(await screen.findByLabelText("Ask the assistant"), "go");
    await user.click(screen.getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(h.onChange).not.toHaveBeenCalled();
  });
});

describe("ModalDescription keyboard (ISS-125)", () => {
  it("submits on Enter and leaves Shift+Enter to the editor", () => {
    const h = setup();
    const enter = new KeyboardEvent("keydown", { key: "Enter" });
    expect(lastProbe().handleKeyDown?.(enter)).toBe(true);
    expect(h.onSubmit).toHaveBeenCalledTimes(1);
    const shift = new KeyboardEvent("keydown", { key: "Enter", shiftKey: true });
    expect(lastProbe().handleKeyDown?.(shift)).toBe(false);
    expect(h.onSubmit).toHaveBeenCalledTimes(1);
  });

  it("leaves other keys alone", () => {
    const h = setup();
    expect(lastProbe().handleKeyDown?.(new KeyboardEvent("keydown", { key: "a" }))).toBe(false);
    expect(h.onSubmit).not.toHaveBeenCalled();
  });

  it("reports edits back as HTML", () => {
    const h = setup();
    act(() => {
      lastProbe().onChange("<p>typed</p>");
    });
    expect(h.onChange).toHaveBeenCalledWith("<p>typed</p>");
  });

  it("carries the dialog's tab position onto the editing surface", () => {
    setup({}, { tabIndex: 2 });
    expect(lastProbe().tabIndex).toBe(2);
  });
});

describe("ModalDescription mentions (ISS-127)", () => {
  it("looks people up for the @-query and inserts the picked one", async () => {
    const user = userEvent.setup();
    const h = setup({
      mentions: [{ member__id: "u-1", member__display_name: "Ada", member__avatar_url: null }],
    });
    act(() => lastProbe().onMentionQuery?.("ad"));
    const option = await screen.findByRole("button", { name: "Ada" });
    expect(h.calls.some((entry) => entry.url.includes("/entity-search/"))).toBe(true);
    await user.click(option);
    expect(screen.queryByRole("button", { name: "Ada" })).not.toBeInTheDocument();
  });

  it("stays quiet when a term finds nobody", async () => {
    setup({ mentions: [] });
    act(() => lastProbe().onMentionQuery?.("zzz"));
    await waitFor(() => expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument());
  });

  it("lists people for the bare @ trigger", async () => {
    setup({ mentions: [{ member__id: "u-1", member__display_name: "Ada", member__avatar_url: null }] });
    act(() => lastProbe().onMentionQuery?.(""));
    expect(await screen.findByRole("button", { name: "Ada" })).toBeInTheDocument();
  });

  it("closes the popup when the query ends", async () => {
    setup({ mentions: [{ member__id: "u-1", member__display_name: "Ada", member__avatar_url: null }] });
    act(() => lastProbe().onMentionQuery?.("ad"));
    await screen.findByRole("button", { name: "Ada" });
    act(() => lastProbe().onMentionQuery?.(null));
    expect(screen.queryByRole("button", { name: "Ada" })).not.toBeInTheDocument();
  });
});

describe("ModalDescription uploads (ISS-127)", () => {
  it("uploads a picked image and reports the parked asset", async () => {
    const h = setup();
    const input = screen.getByLabelText("Attach an image to the description");
    const file = new File(["bytes"], "shot.png", { type: "image/png" });
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() => expect(h.onAssetUploaded).toHaveBeenCalledWith("asset-1"));
    const grant = h.calls.find((entry) => entry.method === "POST" && entry.url.includes("/api/assets/v2/"));
    expect(grant?.body).toMatchObject({ entity_type: "ISSUE_DESCRIPTION", name: "shot.png" });
  });

  it("toasts when the upload is refused", async () => {
    setup({ uploadStatus: 500 });
    const input = screen.getByLabelText("Attach an image to the description");
    fireEvent.change(input, { target: { files: [new File(["b"], "bad.png", { type: "image/png" })] } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't upload the image", expect.any(String)));
  });

  it("ignores a change with no file", () => {
    const h = setup();
    fireEvent.change(screen.getByLabelText("Attach an image to the description"), { target: { files: [] } });
    expect(h.calls.some((entry) => entry.url.includes("/api/assets/v2/"))).toBe(false);
  });
});
