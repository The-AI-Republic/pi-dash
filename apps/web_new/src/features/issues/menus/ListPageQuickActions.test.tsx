// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Whole-list ellipsis (parity/specs/issues.md §7, ISS-067): the two free
// entries, the address it reads from the router, and the edition seam
// that appends entries after them.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const router = vi.hoisted(() => ({ pathname: "/acme/workspace-views/all-issues", searchStr: "" }));

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({ select }: { select: (state: { location: typeof router }) => unknown }) =>
    select({ location: router }),
}));

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { currentPagePath, ListPageQuickActions } from "./ListPageQuickActions.js";
import { registerListMenuExtension, resetListMenuExtension } from "./listMenuExtension.js";
import { LIST_COPY_MESSAGE } from "./messages.js";

const clipboard = { writeText: vi.fn() };

beforeEach(() => {
  vi.clearAllMocks();
  clipboard.writeText.mockResolvedValue(undefined);
  Object.defineProperty(globalThis.navigator, "clipboard", { value: clipboard, configurable: true });
  Object.defineProperty(globalThis, "open", { value: vi.fn(), configurable: true, writable: true });
  router.pathname = "/acme/workspace-views/all-issues";
  router.searchStr = "";
});

afterEach(() => {
  resetListMenuExtension();
});

describe("currentPagePath", () => {
  it("keeps a query string and tolerates a leading question mark", () => {
    expect(currentPagePath("/acme/issues", "")).toBe("/acme/issues");
    expect(currentPagePath("/acme/issues", "?layout=list")).toBe("/acme/issues?layout=list");
    expect(currentPagePath("/acme/issues", "layout=list")).toBe("/acme/issues?layout=list");
  });
});

describe("ListPageQuickActions", () => {
  it("offers open and copy, opening the page's own address", async () => {
    const user = userEvent.setup();
    router.searchStr = "?layout=list";
    render(<ListPageQuickActions />);
    await user.click(screen.getByRole("button", { name: "List actions" }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
    await user.click(items[0]!);
    expect(globalThis.open).toHaveBeenCalledWith(
      "/acme/workspace-views/all-issues?layout=list",
      "_blank",
      "noopener,noreferrer"
    );
  });

  it("copies the list address with the list toast body", async () => {
    const user = userEvent.setup();
    render(<ListPageQuickActions />);
    await user.click(screen.getByRole("button", { name: "List actions" }));
    const items = await screen.findAllByRole("menuitem");
    await user.click(items[1]!);
    expect(notifySuccess).toHaveBeenCalledWith("Link copied", LIST_COPY_MESSAGE);
  });

  it("reports a refused clipboard write", async () => {
    const user = userEvent.setup();
    // The desktop and offline cases throw before the clipboard is even
    // reached; the failure toast carries the reason.
    vi.spyOn(globalThis.navigator, "clipboard", "get").mockReturnValue(undefined as never);
    render(<ListPageQuickActions />);
    await user.click(screen.getByRole("button", { name: "List actions" }));
    const items = await screen.findAllByRole("menuitem");
    await user.click(items[1]!);
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Couldn't copy the list link",
        "The clipboard is not available in this window."
      )
    );
  });

  it("appends edition entries after the built-ins", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const unregister = registerListMenuExtension([{ key: "export", label: "Export CSV", onSelect }]);
    render(<ListPageQuickActions />);
    await user.click(screen.getByRole("button", { name: "List actions" }));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link", "Export CSV"]);
    fireEvent.click(items[2]!);
    expect(onSelect).toHaveBeenCalledTimes(1);

    unregister();
    expect(unregister).toBeTypeOf("function");
  });
});
