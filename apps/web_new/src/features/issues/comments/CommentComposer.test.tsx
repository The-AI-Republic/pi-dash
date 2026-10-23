// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import type { TransportInit } from "@pidash/api-client";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { mentionQueryAt } from "../../../shared/editor/mention.js";
import { notifyError } from "../../../shared/shell/index.js";
import {
  CommentComposer,
  COMPOSER_EXTERNAL_LABEL,
  COMPOSER_INTERNAL_LABEL,
  COMPOSER_LABEL,
} from "./CommentComposer.js";

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
            // exactly as ProseMirror delivers it (CMT-011). The real
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

type HandlerAnswer = { status: number; body: unknown };

function renderComposer(
  props: Partial<React.ComponentProps<typeof CommentComposer>> = {},
  handler: (url: string, init: { method: string; body: string }) => HandlerAnswer | Promise<HandlerAnswer> = () => ({
    status: 200,
    body: {},
  })
) {
  const calls: { method: string; url: string; body: string }[] = [];
  createAppClient("https://api.test", async (url: string, init: TransportInit) => {
    const call = { method: init.method, url, body: init.body ?? "" };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = await handler(url, { method: init.method, body: init.body ?? "" });
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  });
  vi.stubGlobal("fetch", async () => ({ ok: true, status: 204 }));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const onCreate = vi.fn(async () => undefined);
  const view = render(
    <QueryClientProvider client={queryClient}>
      <CommentComposer workspaceSlug="acme" projectId="p-1" showAccess onCreate={onCreate} {...props} />
    </QueryClientProvider>
  );
  return {
    onCreate: (props.onCreate as typeof onCreate | undefined) ?? onCreate,
    calls,
    unmount: view.unmount,
  };
}

async function editor(): Promise<HTMLElement> {
  let box: HTMLElement | null = null;
  await waitFor(() => {
    box = document.querySelector(`textarea[aria-label="${COMPOSER_LABEL}"]`);
    expect(box).not.toBeNull();
  });
  return box as unknown as HTMLElement;
}

afterEach(() => {
  vi.unstubAllGlobals();
  resetAppClient();
  vi.mocked(notifyError).mockClear();
});

// The lazy Tiptap chunk loads once per worker; under the full suite
// that import can outlast the default per-test timeout.
describe("CommentComposer", () => {
  it("posts on Enter with the body and the internal default", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer();
    const box = await editor();
    box.focus();
    await user.keyboard("Hello there");
    await waitFor(() => expect(box).toHaveValue("Hello there"));
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).not.toBeDisabled());
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(onCreate).toHaveBeenCalledWith({ html: "<p>Hello there</p>", access: "INTERNAL", assetIds: [] })
    );
  });

  it("newlines on Shift+Enter and leaves Ctrl/Cmd+Enter inert (CMT-011)", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer();
    const box = await editor();
    box.focus();
    await user.keyboard("one");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    await user.keyboard("{Control>}{Enter}{/Control}");
    await user.keyboard("{Meta>}{Enter}{/Meta}");
    await user.keyboard("two");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onCreate).not.toHaveBeenCalled();
    // Shift added the newline; Ctrl and Cmd added nothing.
    expect(box).toHaveValue("one\ntwo");
    // The real surface delivers these Enters un-prevented (its guard
    // returns false for them); firing from the fake's wrapper div
    // reproduces that delivery, since the newline swallow sits on the
    // textarea itself. The guard must ignore them on its own.
    const wrapper = box.parentElement!;
    fireEvent.keyDown(wrapper, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(wrapper, { key: "Enter", metaKey: true });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onCreate).not.toHaveBeenCalled();
    expect(box).toHaveValue("one\ntwo");
  });

  it("leaves the Enter that commits an IME composition to the input", async () => {
    const { onCreate } = renderComposer();
    const box = await editor();
    box.focus();
    await userEvent.setup().keyboard("im");
    fireEvent.input(box, { target: { value: "imon" } });
    const composing = new KeyboardEvent("keydown", { key: "Enter", bubbles: true });
    Object.defineProperty(composing, "isComposing", { value: true });
    fireEvent(box, composing);
    expect(onCreate).not.toHaveBeenCalled();
  });

  it("does nothing on an empty body", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer();
    await editor();
    expect(screen.getByRole("button", { name: "Comment" })).toBeDisabled();
    await user.keyboard("   ");
    await user.keyboard("{Enter}");
    expect(onCreate).not.toHaveBeenCalled();
  });

  it("ignores a programmatic submit on an empty body", async () => {
    const { onCreate } = renderComposer();
    await editor();
    fireEvent.submit(screen.getByRole("button", { name: "Comment" }).closest("form")!);
    expect(onCreate).not.toHaveBeenCalled();
  });

  it("guards against a double submit while the first post is out", async () => {
    const user = userEvent.setup();
    let release: (() => void) | undefined;
    const onCreate = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    createAppClient("https://api.test", async () => ({
      status: 204,
      url: "",
      headers: { get: () => null },
      text: async () => "",
    }));
    const queryClient = new QueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <CommentComposer workspaceSlug="acme" projectId="p-1" showAccess onCreate={onCreate} />
      </QueryClientProvider>
    );
    const box = await editor();
    box.focus();
    await user.keyboard("Twice");
    await waitFor(() => expect(box).toHaveValue("Twice"));
    await user.keyboard("{Enter}");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole("button", { name: "Comment" }));
    expect(onCreate).toHaveBeenCalledTimes(1);
    release?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).toBeDisabled());
    expect(await editor()).toHaveValue("");
  });

  it("clears the draft and drops parked images when the post fails (CMT-001)", async () => {
    const user = userEvent.setup();
    let attempts = 0;
    const onCreate = vi.fn(async () => {
      attempts += 1;
      if (attempts === 1) throw new Error("nope");
    });
    const { onCreate: created } = renderComposer({ onCreate }, (url, init) => {
      if (init.method === "POST" && url.includes("/projects/p-1/") && !url.includes("/bulk/")) {
        return {
          status: 200,
          body: {
            asset_id: "asset-9",
            asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-9/",
            upload_data: { url: "https://s3.test/upload", fields: {} },
          },
        };
      }
      return { status: 204, body: "" };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("Keep me");
    fireEvent.change(screen.getByLabelText("Add an image to the comment"), {
      target: { files: [new File(["img"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(document.querySelector("img")).not.toBeNull());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(created).toHaveBeenCalledTimes(1));
    expect(created).toHaveBeenCalledWith(expect.objectContaining({ assetIds: ["asset-9"] }));
    // The failed post clears the composer for the next comment.
    expect(await editor()).toHaveValue("");
    expect(document.querySelector("img")).toBeNull();
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).toBeDisabled());
    // The parked image does not leak into the next comment.
    const retry = await editor();
    retry.focus();
    await user.keyboard("After");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(created).toHaveBeenCalledTimes(2));
    expect(created).toHaveBeenLastCalledWith(expect.objectContaining({ assetIds: [] }));
  });

  it("disables submit and holds Enter while an upload is in flight (CMT-011)", async () => {
    const user = userEvent.setup();
    let release!: (answer: { status: number; body: unknown }) => void;
    let held = true;
    const { onCreate } = renderComposer({}, () => {
      if (!held) return { status: 204, body: "" };
      return new Promise((resolve) => {
        release = (answer) => {
          held = false;
          resolve(answer);
        };
      });
    });
    const box = await editor();
    box.focus();
    await user.keyboard("Waiting");
    fireEvent.change(screen.getByLabelText("Add an image to the comment"), {
      target: { files: [new File(["img"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).toBeDisabled());
    await user.keyboard("{Enter}");
    fireEvent.submit(screen.getByRole("button", { name: "Comment" }).closest("form")!);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onCreate).not.toHaveBeenCalled();
    expect(await editor()).toHaveValue("Waiting");
    release({
      status: 200,
      body: {
        asset_id: "asset-9",
        asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-9/",
        upload_data: { url: "https://s3.test/upload", fields: {} },
      },
    });
    // Once the upload lands, Enter posts again.
    await waitFor(() => expect(document.querySelector("img")).not.toBeNull());
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).not.toBeDisabled());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({ assetIds: ["asset-9"] }));
  });

  it("leaves no image node and a usable submit after a failed upload (CMT-009)", async () => {
    const user = userEvent.setup();
    renderComposer({}, () => ({ status: 400, body: { error: "Invalid file type." } }));
    const box = await editor();
    box.focus();
    await user.keyboard("Draft kept");
    fireEvent.change(screen.getByLabelText("Add an image to the comment"), {
      target: { files: [new File(["x"], "bad.svg", { type: "image/svg+xml" })] },
    });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't upload the image", "Invalid file type."));
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(document.querySelector("img")).toBeNull();
    expect(await editor()).toHaveValue("Draft kept");
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).not.toBeDisabled());
  });

  it("offers mention suggestions and posts the picked person (CMT-019)", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer({}, (url) => {
      if (url.includes("/entity-search/")) {
        return {
          status: 200,
          body: {
            user_mention: [{ member__id: "u-1", member__display_name: "Ada L", member__avatar_url: null }],
          },
        };
      }
      return { status: 200, body: {} };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("Hello @ad");
    const popup = await screen.findByRole("list", { name: "Mention suggestions" });
    expect(within(popup).getByText("Users")).toBeInTheDocument();
    expect(within(popup).getByRole("button").textContent).toContain("Ada L");
    await user.keyboard("{Enter}");
    // The pick lands in the draft instead of posting.
    expect(onCreate).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument());
    expect(await editor()).toHaveValue("Hello ");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({ html: expect.stringContaining("mention-component") })
    );
  });

  it("dismisses the mention popup on Escape without posting (CMT-019)", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer({}, (url) => {
      if (url.includes("/entity-search/")) {
        return {
          status: 200,
          body: {
            user_mention: [{ member__id: "u-1", member__display_name: "Ada L", member__avatar_url: null }],
          },
        };
      }
      return { status: 200, body: {} };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("Hello @ad");
    await screen.findByRole("list", { name: "Mention suggestions" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("list", { name: "Mention suggestions" })).not.toBeInTheDocument());
    expect(onCreate).not.toHaveBeenCalled();
    expect(await editor()).toHaveValue("Hello @ad");
  });

  it("shows the access switch only on published projects and posts the choice", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer();
    expect(screen.getByRole("radiogroup", { name: "Comment visibility" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: COMPOSER_INTERNAL_LABEL })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: COMPOSER_EXTERNAL_LABEL }));
    expect(screen.getByRole("radio", { name: COMPOSER_EXTERNAL_LABEL })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: COMPOSER_INTERNAL_LABEL }));
    expect(screen.getByRole("radio", { name: COMPOSER_INTERNAL_LABEL })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: COMPOSER_EXTERNAL_LABEL }));
    const box = await editor();
    box.focus();
    await user.keyboard("Public note");
    await waitFor(() => expect(box).toHaveValue("Public note"));
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(onCreate).toHaveBeenCalledWith({ html: "<p>Public note</p>", access: "EXTERNAL", assetIds: [] })
    );
  });

  it("hides the access switch on projects without a board", async () => {
    renderComposer({ showAccess: false });
    await editor();
    expect(screen.queryByRole("radiogroup", { name: "Comment visibility" })).not.toBeInTheDocument();
  });

  it("uploads an image, embeds it and attaches it with the comment", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer({}, (url, init) => {
      if (init.method === "POST" && url.includes("/projects/p-1/") && !url.includes("/bulk/")) {
        return {
          status: 200,
          body: {
            asset_id: "asset-9",
            asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-9/",
            upload_data: { url: "https://s3.test/upload", fields: { key: "k" } },
          },
        };
      }
      return { status: 204, body: "" };
    });
    const box = await editor();
    box.focus();
    await user.keyboard("See image");
    await waitFor(() => expect(box).toHaveValue("See image"));
    const input = screen.getByLabelText("Add an image to the comment");
    fireEvent.change(input, { target: { files: [new File(["img"], "shot.png", { type: "image/png" })] } });
    await waitFor(() =>
      expect(document.querySelector("img")?.getAttribute("src")).toBe(
        "/api/assets/v2/workspaces/acme/projects/p-1/asset-9/"
      )
    );
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({ assetIds: ["asset-9"] }));
  });

  it("ignores a file picker dismissal and survives a refused upload", async () => {
    const { calls } = renderComposer({}, () => ({ status: 400, body: { error: "Invalid file type." } }));
    await editor();
    const input = screen.getByLabelText("Add an image to the comment");
    fireEvent.change(input, { target: { files: [] } });
    expect(calls).toHaveLength(0);
    fireEvent.change(input, { target: { files: [new File(["x"], "bad.svg", { type: "image/svg+xml" })] } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't upload the image", "Invalid file type."));
  });

  it("posts an image-only body once the upload lands", async () => {
    const { onCreate } = renderComposer({}, (url, init) => {
      if (init.method === "POST" && url.includes("/projects/p-1/") && !url.includes("/bulk/")) {
        return {
          status: 200,
          body: {
            asset_id: "asset-1",
            asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-1/",
            upload_data: { url: "https://s3.test/upload", fields: {} },
          },
        };
      }
      return { status: 204, body: "" };
    });
    await editor();
    expect(screen.getByRole("button", { name: "Comment" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Add an image to the comment"), {
      target: { files: [new File(["img"], "shot.png", { type: "image/png" })] },
    });
    await waitFor(() =>
      expect(document.querySelector("img")?.getAttribute("src")).toBe(
        "/api/assets/v2/workspaces/acme/projects/p-1/asset-1/"
      )
    );
    const submit = screen.getByRole("button", { name: "Comment" });
    await waitFor(() => expect(submit).not.toBeDisabled());
    fireEvent.click(submit);
    await waitFor(() =>
      expect(onCreate).toHaveBeenCalledWith({
        html: '<p><img src="/api/assets/v2/workspaces/acme/projects/p-1/asset-1/"></p>',
        access: "INTERNAL",
        assetIds: ["asset-1"],
      })
    );
  });

  it("keeps the uploaded asset when the editor is gone", async () => {
    const { onCreate, unmount } = renderComposer({}, (url) => {
      if (url.includes("/projects/p-1/") && !url.includes("/bulk/")) {
        return {
          status: 200,
          body: {
            asset_id: "asset-7",
            asset_url: "/api/assets/v2/workspaces/acme/projects/p-1/asset-7/",
            upload_data: { url: "https://s3.test/upload", fields: {} },
          },
        };
      }
      return { status: 204, body: "" };
    });
    await editor();
    const input = screen.getByLabelText("Add an image to the comment");
    unmount();
    fireEvent.change(input, { target: { files: [new File(["img"], "late.png", { type: "image/png" })] } });
    await waitFor(() => expect(notifyError).not.toHaveBeenCalled());
    expect(onCreate).not.toHaveBeenCalled();
  });
}, 20_000);
