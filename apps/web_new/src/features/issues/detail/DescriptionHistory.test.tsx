// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DescriptionHistory, HISTORY_UNKNOWN_AUTHOR, selectVersionAuthor } from "./DescriptionHistory.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const GHOST = "623e4567-e89b-12d3-a456-426614174005";
const V1 = "323e4567-e89b-12d3-a456-426614174002";
const V2 = "723e4567-e89b-12d3-a456-426614174006";

function versionRow(id: string, ownedBy: string, at: string) {
  return {
    id,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    project: PROJECT,
    issue: ISSUE,
    last_saved_at: at,
    owned_by: ownedBy,
    created_at: at,
    updated_at: at,
    created_by: ownedBy,
    updated_by: ownedBy,
  };
}

function versionDetail(id: string, html: string) {
  return {
    ...versionRow(id, ADA, new Date(Date.now() - 3_600_000).toISOString()),
    description_binary: null,
    description_html: html,
    description_stripped: "stripped",
    description_json: {},
  };
}

function membersPayload() {
  return [
    {
      id: "823e4567-e89b-12d3-a456-426614174007",
      member: {
        id: ADA,
        first_name: "Ada",
        last_name: "L",
        avatar: "",
        avatar_url: "https://example.com/ada.png",
        display_name: "Ada L",
        is_bot: false,
      },
      role: 15,
    },
  ];
}

interface StubOptions {
  versions?: "two" | "empty";
  detailStatus?: number;
  holdDetail?: boolean;
}

function renderHistory(props: Partial<React.ComponentProps<typeof DescriptionHistory>> = {}, stub: StubOptions = {}) {
  const onRestore = vi.fn();
  const hourAgo = new Date(Date.now() - 3_600_000).toISOString();
  const dayAgo = new Date(Date.now() - 86_400_000).toISOString();
  const envelope = {
    prev_cursor: "",
    cursor: "",
    next_cursor: null,
    prev_page_results: false,
    next_page_results: false,
    page_count: 1,
    total_results: stub.versions === "empty" ? 0 : 2,
    total_pages: 1,
    results: stub.versions === "empty" ? [] : [versionRow(V1, ADA, hourAgo), versionRow(V2, GHOST, dayAgo)],
  };
  let releaseDetail: (() => void) | null = null;
  createAppClient("https://api.test", async (url: string) => {
    let status = 404;
    let body = "{}";
    if (url.includes(`/description-versions/${V1}/`)) {
      status = stub.detailStatus ?? 200;
      body = JSON.stringify(versionDetail(V1, "<p>version one</p>"));
    } else if (url.includes(`/description-versions/${V2}/`)) {
      status = 200;
      body = JSON.stringify(versionDetail(V2, "<p>version two</p>"));
    } else if (url.includes("description-versions")) {
      status = 200;
      body = JSON.stringify(envelope);
    } else if (url.endsWith("/members/")) {
      status = 200;
      body = JSON.stringify(membersPayload());
    }
    if (stub.holdDetail === true && url.includes(`/description-versions/${V1}/`)) {
      await new Promise<void>((resolve) => {
        releaseDetail = resolve;
      });
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const mounted = render(
    <QueryClientProvider client={queryClient}>
      <DescriptionHistory
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issueId={ISSUE}
        createdAt={dayAgo}
        createdBy={ADA}
        synced={false}
        restoreDisabled={false}
        onRestore={onRestore}
        {...props}
      />
    </QueryClientProvider>
  );
  return { onRestore, releaseDetail: () => releaseDetail?.(), ...mounted };
}

afterEach(() => {
  resetAppClient();
});

describe("selectVersionAuthor", () => {
  it("names listed members and falls back otherwise", () => {
    expect(selectVersionAuthor(membersPayload(), ADA)).toEqual({
      name: "Ada L",
      avatarUrl: "https://example.com/ada.png",
    });
    expect(selectVersionAuthor(membersPayload(), GHOST)).toEqual({
      name: HISTORY_UNKNOWN_AUTHOR,
      avatarUrl: undefined,
    });
    expect(selectVersionAuthor(undefined, ADA)).toEqual({ name: HISTORY_UNKNOWN_AUTHOR, avatarUrl: undefined });
    expect(selectVersionAuthor(membersPayload(), null)).toEqual({
      name: HISTORY_UNKNOWN_AUTHOR,
      avatarUrl: undefined,
    });
    const imageless = [{ ...membersPayload()[0]!, member: { ...membersPayload()[0]!.member, avatar_url: null } }];
    expect(selectVersionAuthor(imageless, ADA)).toEqual({ name: "Ada L", avatarUrl: undefined });
  });
});

describe("DescriptionHistory", () => {
  it("dates the line by the newest version and lists both versions", async () => {
    const user = userEvent.setup();
    renderHistory();
    const trigger = await screen.findByRole("button", { name: /Edited by Ada L 1 hour ago/ });
    await user.click(trigger);
    expect(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" })).toBeInTheDocument();
    expect(await screen.findByRole("menuitem", { name: `${HISTORY_UNKNOWN_AUTHOR} yesterday` })).toBeInTheDocument();
  });

  it("disables the trigger and falls back to the creator without versions", async () => {
    const user = userEvent.setup();
    renderHistory({}, { versions: "empty" });
    const trigger = await screen.findByRole("button", { name: /Edited by Ada L yesterday/ });
    expect(trigger).toBeDisabled();
    await user.click(trigger);
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
  });

  it("annotates synced issues as read-only", async () => {
    renderHistory({ synced: true });
    expect(await screen.findByRole("button", { name: /synced, read-only/ })).toBeInTheDocument();
  });

  it("previews a version with navigation across both", async () => {
    const user = userEvent.setup();
    renderHistory();
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    expect(within(dialog).getByText("Version 1 of 2")).toBeInTheDocument();
    expect(await within(dialog).findByText("version one")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Newer version" })).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: "Older version" }));
    expect(await within(dialog).findByText("version two")).toBeInTheDocument();
    expect(within(dialog).getByText("Version 2 of 2")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Older version" })).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: "Newer version" }));
    expect(await within(dialog).findByText("version one")).toBeInTheDocument();
  });

  it("ignores navigation past the ends", async () => {
    const user = userEvent.setup();
    renderHistory();
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    // fireEvent dispatches where pointer events cannot (disabled
    // buttons): the guards hold the ends instead of crashing.
    fireEvent.click(within(dialog).getByRole("button", { name: "Newer version" }));
    expect(within(dialog).getByText("Version 1 of 2")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Older version" }));
    expect(within(dialog).getByText("Version 2 of 2")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Older version" }));
    expect(within(dialog).getByText("Version 2 of 2")).toBeInTheDocument();
  });

  it("restores the previewed version and closes", async () => {
    const user = userEvent.setup();
    const { onRestore } = renderHistory();
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    await within(dialog).findByText("version one");
    await user.click(within(dialog).getByRole("button", { name: "Restore" }));
    expect(onRestore).toHaveBeenCalledWith("<p>version one</p>");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("hides restore when disabled and closes from Cancel and Escape", async () => {
    const user = userEvent.setup();
    renderHistory({ restoreDisabled: true });
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    await within(dialog).findByText("version one");
    expect(within(dialog).queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    await screen.findByRole("dialog", { name: "Description history" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("holds a skeleton while the version loads", async () => {
    const user = userEvent.setup();
    const { releaseDetail } = renderHistory({}, { holdDetail: true });
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    expect(within(dialog).queryByText("version one")).not.toBeInTheDocument();
    // Portals mount outside the render container, so read the document.
    expect(document.querySelector(".animate-shimmer")).not.toBeNull();
    expect(within(dialog).queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
    releaseDetail();
    await within(dialog).findByText("version one");
  });

  it("reports a version that fails to load", async () => {
    const user = userEvent.setup();
    renderHistory({}, { detailStatus: 500 });
    await user.click(await screen.findByRole("button", { name: /Edited by/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Ada L 1 hour ago" }));
    const dialog = await screen.findByRole("dialog", { name: "Description history" });
    expect(await within(dialog).findByText("Couldn't load this version.")).toBeInTheDocument();
  });
});
