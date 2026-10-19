// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { IssueWidgets } from "./IssueWidgets.js";
import { ISSUE, PROJECT, WS, emptyRelations, newQueryClient, relatedRow, stubTransport } from "./widgetTestUtils.js";

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
});

function renderStack(canEdit = true) {
  const { transport } = stubTransport({
    relations: { ...emptyRelations(), blocked_by: [relatedRow()] },
    links: [],
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

  it("keeps the add controls away from guests", async () => {
    renderStack(false);
    expect(await screen.findByText("Peer work item")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add relation" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add link" })).not.toBeInTheDocument();
  });
});
