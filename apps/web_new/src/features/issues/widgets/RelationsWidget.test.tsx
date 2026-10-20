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
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { RelationsWidget } from "./RelationsWidget.js";
import { RelationActionMenu, WidgetActionsProvider } from "./widgetActions.js";
import {
  ISSUE,
  PEER,
  PROJECT,
  WS,
  emptyRelations,
  newQueryClient,
  relatedRow,
  stubTransport,
} from "./widgetTestUtils.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const navigate = vi.fn();
vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigate,
  Link: ({
    children,
    to,
    params,
  }: {
    children: React.ReactNode;
    to: string;
    params: { ws: string; issueKey: string };
  }) => <a href={to.replace("$ws", params.ws).replace("$issueKey", params.issueKey)}>{children}</a>,
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

function renderWidget(options: Parameters<typeof stubTransport>[0] = {}, canEdit = true) {
  const { calls, transport } = stubTransport(options);
  createAppClient("https://api.test", transport as never);
  const queryClient = newQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WidgetActionsProvider>
        <RelationActionMenu canEdit={canEdit} />
        <RelationsWidget
          workspaceSlug={WS}
          projectId={PROJECT}
          issueId={ISSUE}
          projectIdentifier="WEB"
          canEdit={canEdit}
        />
      </WidgetActionsProvider>
    </QueryClientProvider>
  );
  return { calls, view, queryClient };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(window, "open").mockImplementation(() => null);
  if (document.getElementById(FULL_SCREEN_PORTAL_ID) === null) {
    const portal = document.createElement("div");
    portal.id = FULL_SCREEN_PORTAL_ID;
    document.body.appendChild(portal);
  }
});
afterEach(() => {
  resetAppClient();
  window.localStorage.clear();
  document.getElementById(FULL_SCREEN_PORTAL_ID)?.remove();
  vi.restoreAllMocks();
});

describe("relations widget", () => {
  it("renders only present groups and their rows with the summed count", async () => {
    renderWidget({
      relations: {
        ...emptyRelations(),
        blocked_by: [relatedRow()],
        relates_to: [relatedRow({ id: "823e4567-e89b-12d3-a456-426614174007", name: "Related item" })],
        start_after: [relatedRow({ id: "923e4567-e89b-12d3-a456-426614174008" })],
      },
    });
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    expect(screen.getByText("Related item")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Relations/ })).toHaveTextContent("2");
    expect(screen.queryByTestId("relation-group-duplicate")).not.toBeInTheDocument();
    expect(screen.queryByTestId("relation-group-blocking")).not.toBeInTheDocument();
    expect(screen.getAllByText("WEB-42")).toHaveLength(2);
    expect(screen.getByTestId("relation-group-blocked_by")).toBeInTheDocument();
    expect(screen.getByTestId("relation-group-relates_to")).toBeInTheDocument();
  });

  it("opens the type menu with the four kinds and loads candidates into the dialog", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({
      relations: { ...emptyRelations(), blocked_by: [relatedRow()] },
      candidates: [
        {
          id: "a23e4567-e89b-12d3-a456-426614174009",
          name: "Candidate one",
          sequence_id: 9,
          project__name: "Web",
          project__identifier: "WEB",
          project_id: PROJECT,
          workspace__slug: WS,
          state__name: "Todo",
          state__group: "unstarted",
          state__color: "#000",
          start_date: null,
        },
      ],
    });
    await user.click(await screen.findByRole("button", { name: "Add relation" }));
    for (const label of ["Relates to", "Duplicate of", "Blocked by", "Blocking"]) {
      expect(await screen.findByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    await user.click(screen.getByRole("menuitem", { name: "Blocking" }));
    expect(await screen.findByRole("dialog", { name: /Add blocking/i })).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /Candidate one/ })).toBeInTheDocument();
    expect(calls.some((call) => call.url.includes("search-issues") && call.url.includes("issue_relation=true"))).toBe(
      true
    );
  });

  it("refuses an empty selection with an error notice", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] }, candidates: [] });
    await user.click(await screen.findByRole("button", { name: "Add relation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Blocked by" }));
    await user.click(await screen.findByRole("button", { name: "Add selected work items" }));
    expect(notifyError).toHaveBeenCalledWith("No work items selected", "Pick at least one work item to relate.");
  });

  it("posts the picked candidates under the chosen kind", async () => {
    const user = userEvent.setup();
    const candidate = {
      id: "a23e4567-e89b-12d3-a456-426614174009",
      name: "Candidate one",
      sequence_id: 9,
      project__name: "Web",
      project__identifier: "WEB",
      project_id: PROJECT,
      workspace__slug: WS,
      state__name: "Todo",
      state__group: "unstarted",
      state__color: "#000",
      start_date: null,
    };
    const { calls } = renderWidget({
      relations: { ...emptyRelations(), blocked_by: [relatedRow()] },
      candidates: [candidate],
    });
    await user.click(await screen.findByRole("button", { name: "Add relation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Relates to" }));
    await user.click(await screen.findByRole("option", { name: /Candidate one/ }));
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Relations added"));
    const post = calls.find((call) => call.method === "POST" && call.url.includes("/issue-relation/"));
    expect(post?.body).toContain('"relation_type":"relates_to"');
    expect(post?.body).toContain(candidate.id);
  });

  it("removes a relation through the row menu", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    await user.click(await screen.findByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove relation" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Relation removed"));
    const post = calls.find((call) => call.method === "POST" && call.url.includes("/remove-relation/"));
    expect(post?.body).toContain('"relation_type":"blocked_by"');
    expect(post?.body).toContain(PEER);
  });

  it("paints the removal before the write settles and restores the row on failure", async () => {
    const user = userEvent.setup();
    const options = { relations: { ...emptyRelations(), blocked_by: [relatedRow()] }, writeStatus: 500 };
    const { calls } = renderWidget(options);
    await user.click(await screen.findByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove relation" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't remove the relation", expect.anything()));
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    expect(calls.filter((call) => call.url.includes("/remove-relation/"))).toHaveLength(1);
  });

  it("offers edit, copy, remove and delete with a confirm dialog for editors", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    await user.click(await screen.findByRole("button", { name: "Actions for Peer work item" }));
    expect(await screen.findByRole("menuitem", { name: "Copy link" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Edit" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Remove relation" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Delete" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: /Delete work item/i });
    expect(within(dialog).getByText(/Peer work item/)).toBeInTheDocument();
  });

  it("prefills the row property pickers from the related issue", async () => {
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    expect(await screen.findByRole("button", { name: "High" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Assignees/ })).toBeInTheDocument();
  });

  it("patches a row property through the inline picker", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    await user.click(await screen.findByRole("button", { name: "High" }));
    await user.click(await screen.findByRole("option", { name: "Urgent" }));
    await waitFor(() =>
      expect(calls.some((call) => call.method === "PATCH" && call.body.includes('"priority":"urgent"'))).toBe(true)
    );
  });

  it("opens the peek panel for a row and links to the full item", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    await user.click(await screen.findByText("Peer work item"));
    const panel = await screen.findByTestId("issue-preview");
    expect(within(panel).getByText("WEB-42")).toBeInTheDocument();
    const full = await within(panel).findByText("Open the full work item");
    expect(full).toHaveAttribute("href", `/${WS}/browse/WEB-42`);
  });

  it("opens epics in a new tab instead of the peek panel", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] }, peerIsEpic: true });
    await user.click(await screen.findByText("Peer work item"));
    await waitFor(() =>
      expect(window.open).toHaveBeenCalledWith(`/${WS}/browse/WEB-42`, "_blank", "noopener,noreferrer")
    );
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("shows a load failure inside the peek panel", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] }, peerStatus: 500 });
    await user.click(await screen.findByText("Peer work item"));
    const panel = await screen.findByTestId("issue-preview");
    expect(await within(panel).findByRole("alert")).toHaveTextContent("Couldn't load this work item.");
  });

  it("badges a cross-project row with its own project identifier", async () => {
    const otherProject = "323e4567-e89b-12d3-a456-4266141740aa";
    renderWidget({
      relations: { ...emptyRelations(), blocked_by: [relatedRow({ project_id: otherProject })] },
      extraProjects: [{ id: otherProject, identifier: "OTHER" }],
    });
    expect(await screen.findByText("OTHER-42")).toBeInTheDocument();
    expect(screen.queryByText("WEB-42")).not.toBeInTheDocument();
  });

  it("confirms a delete, then destroys the related work item", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    await user.click(await screen.findByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: /Delete work item/i });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Work item deleted"));
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(PEER))).toBe(true);
  });

  it("reports a failed destroy and closes the confirmation", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] }, writeStatus: 500 });
    await user.click(await screen.findByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: /Delete work item/i });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't delete the work item", expect.anything()));
    expect(screen.queryByRole("dialog", { name: /Delete work item/i })).not.toBeInTheDocument();
  });

  it("renders no section for an editor with no relations", async () => {
    const { view } = renderWidget({ relations: emptyRelations() });
    expect(await screen.findByRole("button", { name: "Add relation" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Relations" })).not.toBeInTheDocument();
    expect(screen.queryByText("No relations yet.")).not.toBeInTheDocument();
    expect(view.container.querySelector("[data-testid='relation-group-blocked_by']")).toBeNull();
  });

  it("adds the first relation from the action row while the section is empty", async () => {
    const user = userEvent.setup();
    const candidate = {
      id: "a23e4567-e89b-12d3-a456-426614174009",
      name: "Candidate one",
      sequence_id: 9,
      project__name: "Web",
      project__identifier: "WEB",
      project_id: PROJECT,
      workspace__slug: WS,
      state__name: "Todo",
      state__group: "unstarted",
      state__color: "#000",
      start_date: null,
    };
    const { calls } = renderWidget({ relations: emptyRelations(), candidates: [candidate] });
    expect(screen.queryByRole("region", { name: "Relations" })).not.toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Add relation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Blocked by" }));
    await user.click(await screen.findByRole("option", { name: /Candidate one/ }));
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Relations added"));
    expect(calls.find((call) => call.method === "POST" && call.url.includes("/issue-relation/"))?.body).toContain(
      '"relation_type":"blocked_by"'
    );
  });

  it("opens the shared work-item form from a row's Edit entry", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    await user.click(await screen.findByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: /Update work item/i }, { timeout: 10_000 });
    expect(within(dialog).getByDisplayValue("Peer work item")).toBeInTheDocument();
  });

  it("destroys an epic through the epic collection", async () => {
    const user = userEvent.setup();
    const { calls } = renderWidget({
      relations: { ...emptyRelations(), blocked_by: [relatedRow()] },
      peerIsEpic: true,
    });
    await user.click(await screen.findByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: /Delete work item/i });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Work item deleted"));
    const destroy = calls.find((call) => call.method === "DELETE");
    expect(destroy?.url).toContain(`/epics/${PEER}/`);
    expect(destroy?.url).not.toContain(`/issues/${PEER}/`);
  });

  it("reports a failed add and keeps the dialog open", async () => {
    const user = userEvent.setup();
    renderWidget({
      relations: emptyRelations(),
      candidates: [
        {
          id: "a23e4567-e89b-12d3-a456-426614174009",
          name: "Candidate one",
          sequence_id: 9,
          project__name: "Web",
          project__identifier: "WEB",
          project_id: PROJECT,
          workspace__slug: WS,
          state__name: "Todo",
          state__group: "unstarted",
          state__color: "#000",
          start_date: null,
        },
      ],
      writeStatus: 500,
    });
    await user.click(await screen.findByRole("button", { name: "Add relation" }));
    await user.click(await screen.findByRole("menuitem", { name: "Relates to" }));
    await user.click(await screen.findByRole("option", { name: /Candidate one/ }));
    await user.click(screen.getByRole("button", { name: "Add selected work items" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't add the relations", expect.anything()));
    expect(screen.getByRole("dialog", { name: /Add relates to/i })).toBeInTheDocument();
  });

  it("keeps only the copy entry in a guest's row menu", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } }, false);
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add relation" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Actions for Peer work item" }));
    expect(await screen.findByRole("menuitem", { name: "Copy link" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Remove relation" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Delete" })).not.toBeInTheDocument();
  });

  it("renders no section for a guest with no relations", async () => {
    renderWidget({}, false);
    expect(await screen.findByRole("button", { name: "Add relation" })).toBeDisabled();
    expect(screen.queryByRole("region", { name: "Relations" })).not.toBeInTheDocument();
  });

  it("hides the group's rows when the inner header is collapsed", async () => {
    const user = userEvent.setup();
    renderWidget({ relations: { ...emptyRelations(), blocked_by: [relatedRow()] } });
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    const header = screen.getByRole("button", { name: /Blocked by/ });
    expect(header).toHaveAttribute("aria-expanded", "true");
    await user.click(header);
    expect(header).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Peer work item")).not.toBeInTheDocument();
    await user.click(header);
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
  });

  it("toggles the section disclosure and remembers it", async () => {
    const user = userEvent.setup();
    const { view } = renderWidget({
      relations: {
        ...emptyRelations(),
        blocked_by: [relatedRow()],
        blocking: [relatedRow({ id: "b23e4567-e89b-12d3-a456-42661417400a", name: "Blocking item" })],
      },
    });
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    expect(screen.getByText("Blocking item")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Relations/ }));
    expect(screen.queryByText("Peer work item")).not.toBeInTheDocument();
    expect(window.localStorage.getItem("pidash:issue-widget-open:Relations")).toBe("false");
    view.unmount();
  });
});
