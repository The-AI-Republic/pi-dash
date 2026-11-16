// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { EPICS_FLAG } from "../edition/flags.js";
import { IssueWidgets } from "./IssueWidgets.js";
import {
  ISSUE,
  PROJECT,
  WS,
  emptyRelations,
  linkRow,
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
  Link: ({ children }: { children: React.ReactNode }) => <a>{children}</a>,
}));

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(() => {
  resetAppClient();
  delete edition.flags[EPICS_FLAG];
});

function renderStack(canEdit = true) {
  const { transport } = stubTransport({
    relations: { ...emptyRelations(), blocked_by: [relatedRow()] },
    links: [linkRow()],
  });
  createAppClient("https://api.test", transport as never);
  render(
    <QueryClientProvider client={newQueryClient()}>
      <IssueWidgets workspaceSlug={WS} projectId={PROJECT} issueId={ISSUE} projectIdentifier="WEB" canEdit={canEdit} />
    </QueryClientProvider>
  );
}

describe("issue widget stack", () => {
  it("mounts the relations and links sections under one labelled region", async () => {
    renderStack();
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Work item details" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Relations/ })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Links/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add relation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add link" })).toBeInTheDocument();
  });

  it("adds the epic create entry only in a cloud build (ISS-232)", async () => {
    renderStack();
    await screen.findByText("Peer work item");
    expect(screen.queryByRole("button", { name: "Create epic" })).not.toBeInTheDocument();
    edition.flags[EPICS_FLAG] = true;
    renderStack();
    expect(await screen.findByRole("button", { name: "Create epic" })).toBeInTheDocument();
  });

  it("keeps the add controls disabled for guests", async () => {
    renderStack(false);
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add relation" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add link" })).toBeDisabled();
  });
});
