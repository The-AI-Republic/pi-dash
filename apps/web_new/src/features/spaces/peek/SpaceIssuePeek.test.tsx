// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardIssueDetail, Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
const notices = vi.hoisted(() => ({ copySuccess: vi.fn(), copyFailure: vi.fn() }));

vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));
vi.mock("../lib/spaceNotices.js", () => ({
  notifySpaceCopySuccess: notices.copySuccess,
  notifySpaceCopyFailure: notices.copyFailure,
}));

import { ApiError, createClient, type ApiClient } from "@pidash/api-client";

import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { STATES, TODO_ID } from "../layouts/testFixtures.js";
import { isGoneError, SpaceIssuePeek } from "./SpaceIssuePeek.js";

const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const ANCHOR = "anchor-1";

function detail(overrides: Partial<SpaceBoardIssueDetail> = {}): SpaceBoardIssueDetail {
  return {
    id: ISSUE_ID,
    name: "Anonymous visitors can read this",
    state_id: TODO_ID,
    sort_order: 1,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: "2020-01-15",
    sequence_id: 7,
    project_id: "22222222-2222-4222-8222-222222222222",
    parent_id: null,
    created_by: null,
    state__group: "unstarted",
    cycle_id: null,
    assignee_ids: [],
    label_ids: [],
    module_ids: [],
    vote_items: [],
    reaction_items: [],
    description_json: null,
    description_html: "<p>Probe <strong>bold</strong> tail</p>",
    description_stripped: "Probe bold tail",
    description_binary: null,
    ...overrides,
  };
}

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

interface SetupOptions {
  body?: () => Promise<Awaited<ReturnType<Transport>>> | Awaited<ReturnType<Transport>>;
  issueId?: string;
  states?: typeof STATES;
  embedded?: boolean;
  /** Keep nodes already in body (the focus-origin scenario). */
  preserveBody?: boolean;
  baseUrl?: string;
}

function setup(options: SetupOptions = {}) {
  const transport: Transport = async (url) => {
    if (url.includes("/issues/")) {
      return await (options.body?.() ?? reply(200, detail()));
    }
    throw new Error(`unexpected request ${url}`);
  };
  const client: ApiClient = createClient({ baseUrl: options.baseUrl ?? "https://api.test", transport });
  runtime.client = client;
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.preserveBody !== true) {
    document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  }
  const onClose = vi.fn();
  const onInvalid = vi.fn();
  const view = render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <SpaceIssuePeek
        anchor={ANCHOR}
        issueId={options.issueId ?? ISSUE_ID}
        projectIdentifier="SPC"
        states={options.states ?? STATES}
        onClose={onClose}
        onInvalid={onInvalid}
        embedded={options.embedded}
      />
    </QueryClientProvider>
  );
  return { onClose, onInvalid, ...view };
}

/** jsdom has no clipboard; install the two members each scenario needs. */
function stubClipboard(permission: "granted" | "denied" | "unsupported") {
  if (permission === "unsupported") {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
    Reflect.deleteProperty(globalThis.navigator, "permissions");
    return;
  }
  const writeText = vi.fn(() => Promise.resolve());
  Object.defineProperty(globalThis.navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
  Object.defineProperty(globalThis.navigator, "permissions", {
    value: { query: () => Promise.resolve({ state: permission === "denied" ? "denied" : "granted" }) },
    configurable: true,
  });
  return writeText;
}

beforeEach(() => {
  notices.copySuccess.mockReset();
  notices.copyFailure.mockReset();
});

afterEach(() => {
  for (const name of ["clipboard", "permissions"] as const) {
    Reflect.deleteProperty(globalThis.navigator, name);
  }
  vi.restoreAllMocks();
});

describe("SpaceIssuePeek shell (SPC-040/041)", () => {
  it("mounts a dialog in the side presentation by default", async () => {
    setup();
    const panel = await screen.findByTestId("space-peek");
    expect(panel).toHaveAttribute("role", "dialog");
    expect(panel).toHaveAttribute("data-peek-layout", "side");
    expect(await screen.findByTestId("space-peek-key")).toHaveTextContent("SPC-7");
    expect(screen.getByRole("heading", { name: "Anonymous visitors can read this" })).toBeInTheDocument();
  });

  it("switches presentations without losing the issue", async () => {
    const user = userEvent.setup();
    setup();
    const panel = await screen.findByTestId("space-peek");
    await screen.findByTestId("space-peek-key");
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Modal" }));
    expect(panel).toHaveAttribute("data-peek-layout", "modal");
    expect(screen.getByTestId("peek-backdrop")).toBeInTheDocument();
    expect(screen.getByTestId("space-peek-key")).toHaveTextContent("SPC-7");
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Full screen" }));
    expect(panel).toHaveAttribute("data-peek-layout", "full");
    expect(screen.getByTestId("space-peek-property-column")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Side panel" }));
    expect(panel).toHaveAttribute("data-peek-layout", "side");
    expect(screen.queryByTestId("space-peek-property-column")).not.toBeInTheDocument();
    expect(screen.getByTestId("space-peek-key")).toHaveTextContent("SPC-7");
  });

  it("moves focus into modal and full and traps Tab between their controls (SPC-041)", async () => {
    const user = userEvent.setup();
    setup();
    const panel = await screen.findByTestId("space-peek");
    await screen.findByTestId("space-peek-key");
    expect(panel).not.toHaveAttribute("aria-modal");
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Modal" }));
    await waitFor(() => {
      expect(panel).toHaveAttribute("aria-modal", "true");
      expect(panel).toContainElement(document.activeElement as HTMLElement);
    });
    const focusables = within(panel).getAllByRole("button");
    focusables[focusables.length - 1]?.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(focusables[0]);
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(focusables[focusables.length - 1]);
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Full screen" }));
    expect(panel).toHaveAttribute("aria-modal", "true");
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Side panel" }));
    expect(panel).not.toHaveAttribute("aria-modal");
    expect(panel).not.toHaveAttribute("tabindex");
  });

  it("keeps Tab inside the panel when it has no focusable child", async () => {
    const user = userEvent.setup();
    setup();
    const panel = await screen.findByTestId("space-peek");
    await screen.findByTestId("space-peek-key");
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Modal" }));
    // Hide the real controls so the empty-list branch is reached.
    for (const node of panel.querySelectorAll("button")) node.setAttribute("hidden", "");
    panel.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(panel);
  });

  it("closes through the header control, Escape and the backdrop", async () => {
    const user = userEvent.setup();
    const first = setup();
    await screen.findByTestId("space-peek-key");
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(first.onClose).toHaveBeenCalledTimes(1);
    first.unmount();

    const second = setup();
    await screen.findByTestId("space-peek-key");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(second.onClose).toHaveBeenCalledTimes(1);
    second.unmount();

    const third = setup();
    await screen.findByTestId("space-peek-key");
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Modal" }));
    fireEvent.pointerDown(document.body);
    expect(third.onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps Escape and outside presses inert for an embedded host", async () => {
    setup({ embedded: true });
    await screen.findByTestId("space-peek-key");
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.pointerDown(document.body);
    expect(screen.getByTestId("space-peek")).toBeInTheDocument();
  });

  it("stands down when nothing focusable opened it", async () => {
    const active = vi.spyOn(document, "activeElement", "get").mockReturnValue(null);
    const view = setup();
    await screen.findByTestId("space-peek-key");
    view.unmount();
    expect(active).toHaveBeenCalled();
  });

  it("does not focus an opener that left the document", async () => {
    document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const view = setup({ preserveBody: true });
    await screen.findByTestId("space-peek-key");
    opener.remove();
    view.unmount();
    expect(document.activeElement).not.toBe(opener);
  });

  it("returns focus to the opener when it unmounts", async () => {
    document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const view = setup({ preserveBody: true });
    await screen.findByTestId("space-peek-key");
    view.unmount();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });
});

describe("isGoneError", () => {
  it("recognises only 403 and 404 API refusals", () => {
    expect(isGoneError(new ApiError({ status: 404, code: "not_found", message: "missing" }))).toBe(true);
    expect(isGoneError(new ApiError({ status: 403, code: "forbidden", message: "private" }))).toBe(true);
    expect(isGoneError(new ApiError({ status: 500, code: "server", message: "down" }))).toBe(false);
    expect(isGoneError(new Error("offline"))).toBe(false);
    expect(isGoneError("nope")).toBe(false);
  });
});

describe("SpaceIssuePeek content (SPC-043/044)", () => {
  it("renders the formatted read-only description and the three properties", async () => {
    setup();
    await screen.findByTestId("space-peek-key");
    const description = screen.getByRole("region", { name: "Issue description" });
    expect(description).toHaveTextContent("Probe bold tail");
    expect(description.querySelector("strong")).not.toBeNull();
    expect(description).not.toHaveAttribute("contenteditable", "true");
    const properties = screen.getByTestId("space-peek-properties");
    expect(within(properties).getByText("Todo")).toBeInTheDocument();
    expect(within(properties).getByText("High")).toBeInTheDocument();
    expect(screen.getByTestId("space-peek-due")).toHaveAttribute("data-overdue", "true");
  });

  it("orders the description before the property panel (SPC-043)", async () => {
    setup();
    await screen.findByTestId("space-peek-key");
    const description = screen.getByRole("region", { name: "Issue description" });
    const properties = screen.getByTestId("space-peek-properties");
    expect(description.compareDocumentPosition(properties) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("renders no description block at all when the description is empty", async () => {
    for (const html of ["", "<p></p>", "   "]) {
      const view = setup({ body: () => reply(200, detail({ description_html: html })) });
      await screen.findByTestId("space-peek-key");
      expect(screen.queryByRole("region", { name: "Issue description" })).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it("resolves description images through the public anchor asset route", async () => {
    setup({
      body: () =>
        reply(
          200,
          detail({ description_html: '<p><image-component src="asset-1" alt="A chart"></image-component></p>' })
        ),
    });
    await screen.findByTestId("space-peek-key");
    const image = screen.getByRole("img", { name: "A chart" });
    expect(image.getAttribute("src")).toBe("https://api.test/api/public/assets/v2/anchor/anchor-1/asset-1/");
  });

  it("keeps asset addresses relative when the client has no base URL", async () => {
    setup({
      baseUrl: "",
      body: () =>
        reply(
          200,
          detail({ description_html: '<p><image-component src="asset-1" alt="A chart"></image-component></p>' })
        ),
    });
    await screen.findByTestId("space-peek-key");
    expect(screen.getByRole("img", { name: "A chart" })).toHaveAttribute(
      "src",
      "/api/public/assets/v2/anchor/anchor-1/asset-1/"
    );
  });
});

describe("SpaceIssuePeek loading and failure (SPC-040/046)", () => {
  it("shows skeletons until the detail resolves, in full too", async () => {
    let release = (): void => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    setup({
      body: async () => {
        await gate;
        return reply(200, detail());
      },
    });
    const user = userEvent.setup();
    expect(await screen.findByTestId("space-peek-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("space-peek-key")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Full screen" }));
    expect(screen.getByTestId("space-peek-skeleton")).toBeInTheDocument();
    release();
    await waitFor(() => {
      expect(screen.queryByTestId("space-peek-skeleton")).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("space-peek-key")).toHaveTextContent("SPC-7");
  });

  it("offers no copy action while the detail skeleton is up (SPC-046)", async () => {
    stubClipboard("granted");
    let release = (): void => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    setup({
      body: async () => {
        await gate;
        return reply(200, detail());
      },
    });
    expect(await screen.findByTestId("space-peek-skeleton")).toBeInTheDocument();
    await waitFor(() => {
      expect(globalThis.navigator.clipboard).toBeDefined();
    });
    expect(screen.queryByRole("button", { name: "Copy link" })).not.toBeInTheDocument();
    release();
    expect(await screen.findByRole("button", { name: "Copy link" })).toBeInTheDocument();
  });

  it("reports a malformed id as invalid without a request", async () => {
    const { onInvalid } = setup({ issueId: "SPC-42" });
    expect(onInvalid).toHaveBeenCalled();
    expect(await screen.findByTestId("space-peek-skeleton")).toBeInTheDocument();
  });

  it("drops peekId for a 404/403 and leaves the board alone", async () => {
    const notFound = setup({ body: () => reply(404, { detail: "missing" }) });
    await waitFor(() => {
      expect(notFound.onInvalid).toHaveBeenCalled();
    });
    notFound.unmount();
    const forbidden = setup({ body: () => reply(403, { detail: "private" }) });
    await waitFor(() => {
      expect(forbidden.onInvalid).toHaveBeenCalled();
    });
  });

  it("shows its own error for a network failure rather than dropping peekId", async () => {
    const { onInvalid } = setup({
      body: () => {
        throw new Error("offline");
      },
    });
    expect(await screen.findByTestId("space-peek-error")).toBeInTheDocument();
    expect(onInvalid).not.toHaveBeenCalled();
  });

  it("shows its own error with a retry for other failures", async () => {
    const user = userEvent.setup();
    let attempts = 0;
    setup({
      body: () => {
        attempts += 1;
        return attempts === 1 ? reply(500, { detail: "down" }) : reply(200, detail());
      },
    });
    expect(await screen.findByTestId("space-peek-error")).toHaveTextContent("This issue could not be loaded.");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByTestId("space-peek-key")).toHaveTextContent("SPC-7");
  });
});

describe("SpaceIssuePeek copy link (SPC-045)", () => {
  it("offers no copy action when clipboard writing is unavailable", async () => {
    stubClipboard("unsupported");
    setup();
    await screen.findByTestId("space-peek-key");
    expect(screen.queryByRole("button", { name: "Copy link" })).not.toBeInTheDocument();
  });

  it("offers no copy action when the permission is denied", async () => {
    stubClipboard("denied");
    setup();
    await screen.findByTestId("space-peek-key");
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Copy link" })).not.toBeInTheDocument();
    });
  });

  it("copies the current address and raises the success notice", async () => {
    const user = userEvent.setup();
    const writeText = stubClipboard("granted");
    setup();
    const copy = await screen.findByRole("button", { name: "Copy link" });
    await user.click(copy);
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(window.location.href);
      expect(notices.copySuccess).toHaveBeenCalled();
    });
    expect(notices.copyFailure).not.toHaveBeenCalled();
  });

  it("raises the failure notice when the write is refused", async () => {
    const user = userEvent.setup();
    Object.defineProperty(globalThis.navigator, "clipboard", {
      value: { writeText: () => Promise.reject(new Error("refused")) },
      configurable: true,
    });
    Object.defineProperty(globalThis.navigator, "permissions", {
      value: { query: () => Promise.resolve({ state: "granted" }) },
      configurable: true,
    });
    setup();
    await user.click(await screen.findByRole("button", { name: "Copy link" }));
    await waitFor(() => {
      expect(notices.copyFailure).toHaveBeenCalled();
    });
  });

  it("raises the failure notice when clipboard access disappears after the probe", async () => {
    const user = userEvent.setup();
    stubClipboard("granted");
    setup();
    const copy = await screen.findByRole("button", { name: "Copy link" });
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
    await user.click(copy);
    expect(notices.copyFailure).toHaveBeenCalled();
  });

  it("offers no copy action in the full presentation", async () => {
    const user = userEvent.setup();
    stubClipboard("granted");
    setup();
    await screen.findByRole("button", { name: "Copy link" });
    await user.click(screen.getByRole("button", { name: "Change presentation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Full screen" }));
    expect(screen.queryByRole("button", { name: "Copy link" })).not.toBeInTheDocument();
  });
});
