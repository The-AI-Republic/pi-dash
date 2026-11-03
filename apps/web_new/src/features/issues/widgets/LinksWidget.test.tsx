// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { LinksWidget, addedAgo, attribution, faviconUrl, linkTitle } from "./LinksWidget.js";
import { LinkActionButton, WidgetActionsProvider } from "./widgetActions.js";
import { LINK, PROJECT, WS, linkRow, newQueryClient, stubTransport } from "./widgetTestUtils.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const ISSUE = "323e4567-e89b-12d3-a456-426614174002";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

function renderWidget(options: Parameters<typeof stubTransport>[0] = {}, canEdit = true) {
  const { calls, transport } = stubTransport(options);
  createAppClient("https://api.test", transport as never);
  const queryClient = newQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WidgetActionsProvider>
        <LinkActionButton canEdit={canEdit} />
        <LinksWidget workspaceSlug={WS} projectId={PROJECT} issueId={ISSUE} canEdit={canEdit} />
      </WidgetActionsProvider>
    </QueryClientProvider>
  );
  return { calls, view };
}

function stubClipboard() {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  return writeText;
}

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(() => {
  resetAppClient();
  window.localStorage.clear();
  vi.restoreAllMocks();
});

describe("link display helpers", () => {
  it("derives a favicon from the URL origin and refuses malformed strings", () => {
    expect(faviconUrl("https://example.com/doc?x=1")).toBe("https://example.com/favicon.ico");
    expect(faviconUrl("not a url")).toBeNull();
  });

  it("falls back from a blank title to the URL", () => {
    expect(linkTitle(linkRow({ title: "Design doc" }))).toBe("Design doc");
    expect(linkTitle(linkRow({ title: "   " }))).toBe("https://example.com/doc");
    expect(linkTitle(linkRow({ title: null }))).toBe("https://example.com/doc");
  });

  it("formats relative ages across the minute, hour, day, month and year bands", () => {
    const now = Date.parse("2024-06-01T12:00:00Z");
    expect(addedAgo("2024-06-01T11:59:30Z", now)).toBe("just now");
    expect(addedAgo("2024-06-01T11:30:00Z", now)).toBe("30 minutes ago");
    expect(addedAgo("2024-06-01T09:00:00Z", now)).toBe("3 hours ago");
    expect(addedAgo("2024-05-30T12:00:00Z", now)).toBe("2 days ago");
    expect(addedAgo("2024-03-01T12:00:00Z", now)).toBe("3 months ago");
    expect(addedAgo("2022-05-01T12:00:00Z", now)).toBe("2 years ago");
    expect(addedAgo("nonsense", now)).toBe("just now");
  });

  it("labels bots and omits an unknown author", () => {
    const now = Date.parse("2024-06-01T12:00:00Z");
    const bot = linkRow({
      created_at: "2024-06-01T11:00:00Z",
      created_by_detail: {
        id: "723e4567-e89b-12d3-a456-426614174006",
        first_name: "Ada",
        last_name: "L",
        avatar: "",
        avatar_url: null,
        is_bot: true,
        display_name: "Ada L",
      },
    });
    expect(attribution(bot, now)).toBe("Added 1 hour ago by Ada L (bot)");
    expect(attribution(linkRow({ created_at: "2024-06-01T11:00:00Z", created_by_detail: null }), now)).toBe(
      "Added 1 hour ago"
    );
  });
});

describe("links widget", () => {
  it("renders each link with its title, attribution and new-tab anchor", async () => {
    renderWidget({
      links: [
        linkRow(),
        linkRow({
          id: "623e4567-e89b-12d3-a456-426614174005",
          title: null,
          url: "https://example.com/bare",
          created_at: "2024-01-01T00:00:00Z",
        }),
      ],
    });
    expect(await screen.findByText("Design doc")).toBeInTheDocument();
    const bare = screen.getByText("https://example.com/bare");
    expect(within(bare.closest("a") as HTMLElement).getByText("https://example.com/bare")).toBeInTheDocument();
    expect(bare.closest("a")).toHaveAttribute("target", "_blank");
    expect(bare.closest("a")).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getAllByText(/Added .* by Ada L/).length).toBeGreaterThan(0);
  });

  it("draws the globe fallback for a link whose URL cannot be parsed", async () => {
    renderWidget({ links: [linkRow({ url: "not a url" })] });
    const row = await screen.findByTestId(/link-row-/);
    expect(row.querySelector("img")).toBeNull();
    expect(row.querySelector("svg")).not.toBeNull();
  });

  it("closes the dialog and clears the edit target", async () => {
    const user = userEvent.setup();
    renderWidget({ links: [linkRow()] });
    await user.click(await screen.findByRole("button", { name: "Link actions for Design doc" }));
    await user.click(await screen.findByRole("menuitem", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit link" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Edit link" })).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(await screen.findByRole("dialog", { name: "Add link" })).toBeInTheDocument();
  });

  it("orders rows newest first even when the read is unordered", async () => {
    renderWidget({
      links: [
        linkRow({ id: "123e4567-e89b-12d3-a456-426614174001", title: "Older", created_at: "2024-01-01T00:00:00Z" }),
        linkRow({ id: "223e4567-e89b-12d3-a456-426614174002", title: "Newer", created_at: "2024-01-03T00:00:00Z" }),
      ],
    });
    await screen.findByText("Newer");
    const rows = screen.getAllByTestId(/link-row-/);
    expect(rows[0]).toHaveTextContent("Newer");
  });

  it("copies a link's URL with a toast", async () => {
    const user = userEvent.setup();
    const writeText = stubClipboard();
    renderWidget({ links: [linkRow()] });
    await user.click(await screen.findByRole("button", { name: /Copy link Design doc/ }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("https://example.com/doc"));
    expect(notifySuccess).toHaveBeenCalledWith("Link copied");
  });

  it("reports a clipboard failure without losing the row", async () => {
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    renderWidget({ links: [linkRow()] });
    await user.click(await screen.findByRole("button", { name: /Copy link Design doc/ }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't copy the link", "denied"));
    expect(screen.getByText("Design doc")).toBeInTheDocument();
  });

  it("creates a link through the shared modal and normalizes the URL", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ links: [] });
    await user.click(await screen.findByRole("button", { name: "Add link" }));
    await user.type(screen.getByLabelText("URL"), "example.com/new");
    await user.type(screen.getByLabelText("Display title (optional)"), "New doc");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link added"));
    const post = calls.find((call) => call.method === "POST" && call.url.includes("/issue-links/"));
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ url: "http://example.com/new", title: "New doc" });
  });

  it("edits a link through the same modal and patches only that link", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ links: [linkRow()] });
    await user.click(await screen.findByRole("button", { name: "Link actions for Design doc" }));
    await user.click(await screen.findByRole("menuitem", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit link" });
    const title = within(dialog).getByLabelText("Display title (optional)");
    await user.clear(title);
    await user.type(title, "Renamed");
    await user.click(within(dialog).getByRole("button", { name: "Save link" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link updated"));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(LINK);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ url: "https://example.com/doc", title: "Renamed" });
  });

  it("clears the title when the edit leaves it empty", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ links: [linkRow()] });
    await user.click(await screen.findByRole("button", { name: "Link actions for Design doc" }));
    await user.click(await screen.findByRole("menuitem", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit link" });
    await user.clear(within(dialog).getByLabelText("Display title (optional)"));
    await user.click(within(dialog).getByRole("button", { name: "Save link" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link updated"));
    expect(JSON.parse(calls.find((call) => call.method === "PATCH")?.body ?? "{}")).toEqual({
      url: "https://example.com/doc",
      title: null,
    });
  });

  it("removes a link immediately, with no confirmation", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ links: [linkRow()] });
    await user.click(await screen.findByRole("button", { name: "Link actions for Design doc" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link removed"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(LINK))).toBe(true);
  });

  it("reports a failed create and keeps the modal open", async () => {
    const user = userEvent.setup();
    renderWidget({ links: [], writeStatus: 400 });
    await user.click(await screen.findByRole("button", { name: "Add link" }));
    await user.type(screen.getByLabelText("URL"), "example.com/new");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't add the link", expect.anything()));
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
  });

  it("reports a failed edit and keeps the modal open", async () => {
    const user = userEvent.setup();
    renderWidget({ links: [linkRow()], writeStatus: 400 });
    await user.click(await screen.findByRole("button", { name: "Link actions for Design doc" }));
    await user.click(await screen.findByRole("menuitem", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit link" });
    await user.click(within(dialog).getByRole("button", { name: "Save link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't update the link", expect.anything()));
    expect(screen.getByRole("dialog", { name: "Edit link" })).toBeInTheDocument();
  });

  it("reports a failed removal and keeps the row", async () => {
    const user = userEvent.setup();
    renderWidget({ links: [linkRow()], writeStatus: 400 });
    await user.click(await screen.findByRole("button", { name: "Link actions for Design doc" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't remove the link", expect.anything()));
    expect(screen.getByText("Design doc")).toBeInTheDocument();
  });

  it("hides add and row controls from guests but keeps the anchor and copy", async () => {
    renderWidget({ links: [linkRow()] }, false);
    expect(await screen.findByText("Design doc")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add link" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Link actions for Design doc" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Copy link Design doc/ })).toBeInTheDocument();
  });

  it("renders no section for a guest with no links", async () => {
    renderWidget({ links: [] }, false);
    expect(await screen.findByRole("button", { name: "Add link" })).toBeDisabled();
    expect(screen.queryByRole("region", { name: "Links" })).not.toBeInTheDocument();
  });

  it("adds the first link from the action row while the section is empty", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ links: [] });
    expect(screen.queryByRole("region", { name: "Links" })).not.toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Add link" }));
    await user.type(await screen.findByLabelText("URL"), "example.com/first");
    await user.click(screen.getByRole("button", { name: "Add link" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link added"));
    expect(calls.find((call) => call.method === "POST" && call.url.includes("/issue-links/"))?.body).toContain(
      "http://example.com/first"
    );
  });

  it("renders no section for an editor with no links", async () => {
    renderWidget({ links: [] });
    expect(await screen.findByRole("button", { name: "Add link" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Links" })).not.toBeInTheDocument();
    expect(screen.queryByText("No links yet.")).not.toBeInTheDocument();
  });

  it("toggles the section disclosure and remembers it", async () => {
    const user = userEvent.setup();
    const { view } = renderWidget({ links: [linkRow()] });
    expect(await screen.findByText("Design doc")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Links/ }));
    expect(screen.queryByText("Design doc")).not.toBeInTheDocument();
    expect(window.localStorage.getItem("pidash:issue-widget-open:Links")).toBe("false");
    view.unmount();
  });
});
