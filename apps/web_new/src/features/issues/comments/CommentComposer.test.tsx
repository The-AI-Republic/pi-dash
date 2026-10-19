// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import type { TransportInit } from "@pidash/api-client";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
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
  }: {
    initialHtml: string;
    onChange: (html: string) => void;
    label: string;
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
          defaultValue={initialHtml.replace(/<[^>]*>/g, "").trim()}
          onInput={(event) => onChange(`<p>${event.currentTarget.value}</p>`)}
        />
      </div>
    );
  }
  return { ...original, RichTextEditor: FakeEditor, CommentRichTextEditor: FakeEditor };
});

function renderComposer(
  props: Partial<React.ComponentProps<typeof CommentComposer>> = {},
  handler: (url: string, init: { method: string; body: string }) => { status: number; body: unknown } = () => ({
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
    const answer = handler(url, { method: init.method, body: init.body ?? "" });
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
  render(
    <QueryClientProvider client={queryClient}>
      <CommentComposer workspaceSlug="acme" projectId="p-1" showAccess onCreate={onCreate} {...props} />
    </QueryClientProvider>
  );
  return { onCreate: (props.onCreate as typeof onCreate | undefined) ?? onCreate, calls };
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

  it("inserts a line break on modifier and shift plus Enter", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer();
    const box = await editor();
    box.focus();
    await user.keyboard("one");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    await user.keyboard("{Control>}{Enter}{/Control}");
    await user.keyboard("two");
    await new Promise((resolve) => setTimeout(resolve, 50));
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
    release?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).toBeDisabled());
    expect(await editor()).toHaveValue("");
  });

  it("keeps the draft when the post fails", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer({ onCreate: vi.fn(async () => Promise.reject(new Error("nope"))) });
    const box = await editor();
    box.focus();
    await user.keyboard("Keep me");
    await waitFor(() => expect(box).toHaveValue("Keep me"));
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("button", { name: "Comment" })).not.toBeDisabled());
    expect(await editor()).toHaveValue("Keep me");
  });

  it("shows the access switch only on published projects and posts the choice", async () => {
    const user = userEvent.setup();
    const { onCreate } = renderComposer();
    expect(screen.getByRole("radiogroup", { name: "Comment visibility" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: COMPOSER_INTERNAL_LABEL })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: COMPOSER_EXTERNAL_LABEL }));
    expect(screen.getByRole("radio", { name: COMPOSER_EXTERNAL_LABEL })).toHaveAttribute("aria-checked", "true");
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
}, 20_000);
