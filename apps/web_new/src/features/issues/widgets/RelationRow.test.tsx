// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { WORKFLOW_FLAG } from "../edition/flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "../edition/providers.js";
import { RelationRow, copyWorkItemUrl } from "./RelationRow.js";
import { PEER, PICKER_STATES, PROJECT, WS, newQueryClient, relatedRow, stubTransport } from "./widgetTestUtils.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

function renderRow(
  canEdit = true,
  options: Parameters<typeof stubTransport>[0] = {},
  rowOverrides: Parameters<typeof relatedRow>[0] = {}
) {
  const { calls, transport } = stubTransport({ states: PICKER_STATES, ...options });
  createAppClient("https://api.test", transport as never);
  const handlers = {
    onOpen: vi.fn(),
    onEdit: vi.fn(),
    onRemove: vi.fn(),
    onDelete: vi.fn(),
  };
  render(
    <QueryClientProvider client={newQueryClient()}>
      <RelationRow
        workspaceSlug={WS}
        projectIdentifier="WEB"
        row={relatedRow(rowOverrides)}
        canEdit={canEdit}
        {...handlers}
      />
    </QueryClientProvider>
  );
  return { calls, ...handlers };
}

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(() => {
  resetAppClient();
  resetEditionIssuesProviders();
  delete edition.flags[WORKFLOW_FLAG];
  vi.restoreAllMocks();
});

describe("relation row", () => {
  it("shows the identifier and name and opens on the identity button", async () => {
    const user = userEvent.setup();
    const { onOpen } = renderRow();
    expect(screen.getByText("WEB-42")).toBeInTheDocument();
    await user.click(screen.getByText("Peer work item"));
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: PEER }));
  });

  it("patches a property and refreshes the widget read", async () => {
    const user = userEvent.setup();
    const { calls } = renderRow();
    await user.click(screen.getByRole("button", { name: "High" }));
    await user.click(await screen.findByRole("option", { name: "Urgent" }));
    await waitFor(() =>
      expect(calls.some((call) => call.method === "PATCH" && call.body.includes('"priority":"urgent"'))).toBe(true)
    );
  });

  it("patches an epic through the epic collection", async () => {
    const user = userEvent.setup();
    const { calls } = renderRow(true, { peerIsEpic: true });
    await user.click(screen.getByRole("button", { name: "High" }));
    await user.click(await screen.findByRole("option", { name: "Urgent" }));
    await waitFor(() =>
      expect(calls.some((call) => call.method === "PATCH" && call.url.includes(`/epics/${PEER}/`))).toBe(true)
    );
    expect(calls.some((call) => call.method === "PATCH" && call.url.includes(`/issues/${PEER}/`))).toBe(false);
  });

  it("reports a failed property write and keeps the row", async () => {
    const user = userEvent.setup();
    renderRow(true, { writeStatus: 500 });
    await user.click(screen.getByRole("button", { name: "High" }));
    await user.click(await screen.findByRole("option", { name: "Urgent" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't update the related work item", expect.anything())
    );
    expect(screen.getByText("Peer work item")).toBeInTheDocument();
  });

  it("disables the property pickers for guests", () => {
    renderRow(false);
    expect(screen.getByRole("button", { name: "High" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Assignees/ })).toBeDisabled();
  });

  it("fires the row menu callbacks for editors", async () => {
    const user = userEvent.setup();
    const { onEdit, onRemove, onDelete } = renderRow();
    await user.click(screen.getByRole("button", { name: "Actions for Peer work item" }));
    expect(await screen.findByRole("menuitem", { name: "Copy link" })).toBeInTheDocument();
    await user.click(await screen.findByRole("menuitem", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledWith(expect.objectContaining({ id: PEER }));
    await user.click(screen.getByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove relation" }));
    expect(onRemove).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalled();
  });

  it("copies the row URL with a toast", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    await copyWorkItemUrl("/acme/browse/WEB-42");
    expect(writeText).toHaveBeenCalledWith(new URL("/acme/browse/WEB-42", window.location.origin).toString());
    expect(notifySuccess).toHaveBeenCalledWith("Link copied");
  });

  it("reports a clipboard failure", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    await copyWorkItemUrl("/acme/browse/WEB-42");
    expect(notifyError).toHaveBeenCalledWith("Couldn't copy the link", "denied");
  });

  it("copies through the row menu for guests", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    renderRow(false);
    await user.click(screen.getByRole("button", { name: "Actions for Peer work item" }));
    await user.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(new URL(`/${WS}/browse/WEB-42`, window.location.origin).toString())
    );
  });
});

describe("relation row workflow gate (ISS-233)", () => {
  const ROW_STATE = PICKER_STATES[0]!.id;
  const REFUSED = PICKER_STATES[1]!.id;

  it("keeps every state open in the community build", async () => {
    const user = userEvent.setup();
    registerEditionIssuesProviders({
      workflowRules: { refusal: () => ({ reason: "no rule should be asked" }) },
    });
    renderRow(true, {}, { state_id: ROW_STATE });
    await user.click(screen.getByRole("button", { name: "Select state" }));
    const options = await screen.findAllByRole("option");
    expect(options.every((option) => !(option as HTMLButtonElement).disabled)).toBe(true);
  });

  it("greys the states the rules refuse from the row's own state", async () => {
    const user = userEvent.setup();
    edition.flags[WORKFLOW_FLAG] = true;
    registerEditionIssuesProviders({
      workflowRules: {
        refusal: ({ toStateId }) => (toStateId === REFUSED ? { reason: "Finish the parent first" } : null),
      },
    });
    renderRow(true, {}, { state_id: ROW_STATE });
    await user.click(screen.getByRole("button", { name: "Select state" }));
    const refused = await screen.findByRole("option", { name: /Done/ });
    expect(refused).toBeDisabled();
    expect(refused).toHaveTextContent("Finish the parent first");
    expect(screen.getByRole("option", { name: "Todo" })).toBeEnabled();
  });
});

describe("relation row with no states", () => {
  it("opens an empty state list instead of failing when the project has none", async () => {
    const user = userEvent.setup();
    const { transport } = stubTransport({});
    createAppClient("https://api.test", transport as never);
    render(
      <QueryClientProvider client={newQueryClient()}>
        <RelationRow
          workspaceSlug={WS}
          projectIdentifier="WEB"
          row={relatedRow()}
          canEdit
          onOpen={vi.fn()}
          onEdit={vi.fn()}
          onRemove={vi.fn()}
          onDelete={vi.fn()}
        />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Select state" }));
    expect(await screen.findByText("No matching results")).toBeInTheDocument();
  });
});

describe("relation row project badge", () => {
  it("uses the project identifier passed by the widget", () => {
    renderRow();
    expect(screen.getByText(`${"WEB"}-42`)).toBeInTheDocument();
    expect(screen.getByText("Peer work item")).toBeInTheDocument();
    expect(PROJECT).toBe("223e4567-e89b-12d3-a456-426614174001");
  });
});
