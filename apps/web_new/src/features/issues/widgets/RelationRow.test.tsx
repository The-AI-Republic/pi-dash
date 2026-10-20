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
import { RelationRow, copyWorkItemUrl } from "./RelationRow.js";
import { PEER, PROJECT, WS, newQueryClient, relatedRow, stubTransport } from "./widgetTestUtils.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

function renderRow(canEdit = true, options: Parameters<typeof stubTransport>[0] = {}) {
  const { calls, transport } = stubTransport(options);
  createAppClient("https://api.test", transport as never);
  const handlers = {
    onOpen: vi.fn(),
    onEdit: vi.fn(),
    onRemove: vi.fn(),
    onDelete: vi.fn(),
  };
  render(
    <QueryClientProvider client={newQueryClient()}>
      <RelationRow workspaceSlug={WS} projectIdentifier="WEB" row={relatedRow()} canEdit={canEdit} {...handlers} />
    </QueryClientProvider>
  );
  return { calls, ...handlers };
}

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(() => {
  resetAppClient();
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

describe("relation row project badge", () => {
  it("uses the project identifier passed by the widget", () => {
    renderRow();
    expect(screen.getByText(`${"WEB"}-42`)).toBeInTheDocument();
    expect(screen.getByText("Peer work item")).toBeInTheDocument();
    expect(PROJECT).toBe("223e4567-e89b-12d3-a456-426614174001");
  });
});
