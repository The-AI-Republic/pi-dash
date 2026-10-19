// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { clearCommands, listCommands } from "../../../shared/commands/index.js";
import { CREATE_WORK_ITEM_COMMAND_ID, WorkItemCreateEntry } from "./WorkItemCreateEntry.js";

const PROJECTS = [
  { id: "p1", name: "Atlas", identifier: "ATL", memberRole: 15, archived: false },
  { id: "p2", name: "Beacon", identifier: "BCN", memberRole: 20, archived: false },
  { id: "p3", name: "Guestland", identifier: "GST", memberRole: 5, archived: false },
  { id: "p4", name: "Old Atlas", identifier: "OLD", memberRole: 20, archived: true },
];

function setup(
  props: Partial<React.ComponentProps<typeof WorkItemCreateEntry>> = {}
): React.ComponentProps<typeof WorkItemCreateEntry> {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => (url.endsWith("/auth/get-csrf-token/") ? '{"csrf_token":"csrf"}' : "{}"),
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const resolved = {
    workspaceSlug: "acme",
    projects: PROJECTS,
    canCreate: true,
    initialProjectId: null,
    ...props,
  };
  render(
    <QueryClientProvider client={queryClient}>
      <WorkItemCreateEntry {...resolved} />
    </QueryClientProvider>
  );
  return resolved;
}

function command() {
  return listCommands().find((entry) => entry.id === CREATE_WORK_ITEM_COMMAND_ID);
}

afterEach(() => {
  resetAppClient();
  clearCommands();
});

describe("WorkItemCreateEntry", () => {
  it("registers the New work item command on the ni sequence without a project hint", () => {
    setup();
    expect(command()?.title).toBe("New work item");
    expect(command()?.sequence).toBe("ni");
    expect(command()?.section).toBe("Create");
    expect(command()?.hint).toBeUndefined();
    expect(command()?.keywords).toEqual(["create", "add", "new", "issue"]);
  });

  it("hints the page's own project when one is in view", () => {
    setup({ initialProjectId: "p2" });
    expect(command()?.hint).toBe("Beacon");
  });

  it("omits the hint for a page project the member may not file into", () => {
    setup({ initialProjectId: "p3" });
    expect(command()?.hint).toBeUndefined();
  });

  it("opens the shared create dialog from the command", async () => {
    setup();
    await act(async () => {
      await command()?.run();
    });
    expect(await screen.findByRole("dialog", { name: "Create new work item" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toBeInTheDocument();
  });

  it("opens on the page's project when there is one", async () => {
    setup({ initialProjectId: "p2" });
    await act(async () => {
      await command()?.run();
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Beacon" })).toBeInTheDocument());
  });

  it("registers nothing for a guest", () => {
    setup({ canCreate: false });
    expect(command()).toBeUndefined();
  });

  it("registers nothing when every joined project is a guest membership or archived", () => {
    setup({ projects: [PROJECTS[2]!, PROJECTS[3]!] });
    expect(command()).toBeUndefined();
  });

  it("drops the command when the entry unmounts", () => {
    createAppClient("https://api.test", (async (url: string) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    })) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = render(
      <QueryClientProvider client={queryClient}>
        <WorkItemCreateEntry workspaceSlug="acme" projects={PROJECTS} canCreate initialProjectId={null} />
      </QueryClientProvider>
    );
    expect(command()).toBeDefined();
    view.unmount();
    expect(command()).toBeUndefined();
  });

  it("re-registers beside another screen's commands rather than replacing them", async () => {
    const { registerCommands } = await import("../../../shared/commands/index.js");
    const other = registerCommands([
      { id: "issues.refresh-list", title: "Refresh issue list", section: "Issues", run: () => undefined },
    ]);
    setup();
    expect(
      listCommands()
        .map((entry) => entry.id)
        .sort()
    ).toEqual([CREATE_WORK_ITEM_COMMAND_ID, "issues.refresh-list"].sort());
    other();
  });

  it("does not open the dialog before the command runs", () => {
    setup();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes the dialog from its close control and can open it again", async () => {
    setup();
    await act(async () => {
      await command()?.run();
    });
    fireEvent.click(await screen.findByRole("button", { name: "Close dialog" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Create new work item" })).not.toBeInTheDocument());
    await act(async () => {
      await command()?.run();
    });
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
  });
});
