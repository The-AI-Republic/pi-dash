// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DOCS_URL } from "./entries/account.js";
import {
  CHANGELOG_LOAD_TIMEOUT_MS,
  CHANGELOG_URL,
  changelogCategoryUrl,
  SUPPORT_EMAIL_URL,
  UpdatesDialog,
} from "./UpdatesDialog.js";

function setup(overrides: Record<string, unknown> = {}) {
  const handlers = { onOpenChange: vi.fn(), onOpenExternal: vi.fn() };
  render(
    <UpdatesDialog
      open
      version="1.3.0"
      changelogUrl="https://changelog.example/feed"
      selfManaged={false}
      {...handlers}
      {...overrides}
    />
  );
  return handlers;
}

describe("changelogCategoryUrl", () => {
  it("flavours the changelog address per edition", () => {
    expect(changelogCategoryUrl(false)).toBe("https://airepublic.com/changelog?category=cloud");
    expect(changelogCategoryUrl(true)).toBe("https://airepublic.com/changelog?category=self-hosted");
  });
});

describe("UpdatesDialog", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("heads the dialog with the running version", () => {
    setup();
    expect(screen.getByRole("dialog", { name: "What's new?" })).toBeInTheDocument();
    expect(screen.getByText("Version: v1.3.0")).toBeInTheDocument();
  });

  it("embeds the configured changelog behind its loader", () => {
    setup();
    const frame = screen.getByTitle("Product changelog") as HTMLIFrameElement;
    expect(frame.getAttribute("src")).toBe("https://changelog.example/feed");
    expect(screen.getByText("Loading updates…")).toBeInTheDocument();
    fireEvent.load(frame);
    expect(screen.queryByText("Loading updates…")).not.toBeInTheDocument();
  });

  it("falls back when the embed errors", () => {
    const handlers = setup();
    fireEvent.error(screen.getByTitle("Product changelog"));
    expect(screen.queryByTitle("Product changelog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Go to changelog" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith("https://airepublic.com/changelog?category=cloud");
  });

  it("falls back when the embed never settles", () => {
    vi.useFakeTimers();
    setup();
    expect(screen.getByTitle("Product changelog")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(CHANGELOG_LOAD_TIMEOUT_MS);
    });
    expect(screen.queryByTitle("Product changelog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Go to changelog" })).toBeInTheDocument();
  });

  it("leaves a loaded embed alone past the timeout", () => {
    vi.useFakeTimers();
    setup();
    fireEvent.load(screen.getByTitle("Product changelog"));
    act(() => {
      vi.advanceTimersByTime(CHANGELOG_LOAD_TIMEOUT_MS * 2);
    });
    expect(screen.getByTitle("Product changelog")).toBeInTheDocument();
  });

  it("falls back without a configured changelog, flavoured per edition", () => {
    const handlers = setup({ changelogUrl: null, selfManaged: true });
    expect(screen.queryByTitle("Product changelog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Go to changelog" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith("https://airepublic.com/changelog?category=self-hosted");
  });

  it("falls back on an empty changelog leg too", () => {
    setup({ changelogUrl: "" });
    expect(screen.queryByTitle("Product changelog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Go to changelog" })).toBeInTheDocument();
  });

  it("links docs, the full changelog and support from the footer", () => {
    const handlers = setup();
    fireEvent.click(screen.getByRole("button", { name: "Docs" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith(DOCS_URL);
    fireEvent.click(screen.getByRole("button", { name: "Full changelog" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith(CHANGELOG_URL);
    fireEvent.click(screen.getByRole("button", { name: "Support" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith(SUPPORT_EMAIL_URL);
  });

  it("ignores Escape and closes from the close button", async () => {
    const user = userEvent.setup();
    const handlers = setup();
    await user.keyboard("{Escape}");
    expect(handlers.onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "What's new?" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(handlers.onOpenChange).toHaveBeenCalledWith(false, expect.anything());
  });
});
