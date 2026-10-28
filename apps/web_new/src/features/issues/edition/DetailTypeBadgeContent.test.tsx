// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud type badge body (ISS-235): names the issue's work-item type
// beside the identifier, and stays hidden for an untyped item, an
// unknown type, or a build without the flag or the type source.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getIssue: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  getIssue: mocks.getIssue,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { edition } from "../../../core/edition/index.js";
import { DetailTypeBadgeContent, selectTypeBadgeName } from "./DetailTypeBadgeContent.js";
import { ISSUE_TYPES_FLAG } from "./flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";

afterEach(() => {
  cleanup();
  delete edition.flags[ISSUE_TYPES_FLAG];
  resetEditionIssuesProviders();
  mocks.getIssue.mockReset();
});

function renderBadge(): void {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <DetailTypeBadgeContent workspaceSlug="acme" projectId="p-1" issueId="i-1" editable={false} />
    </QueryClientProvider>
  );
}

describe("selectTypeBadgeName", () => {
  it("names a listed type", () => {
    expect(selectTypeBadgeName("t-1", [{ id: "t-1", name: "Bug" }])).toBe("Bug");
  });

  it("hides an untyped item", () => {
    expect(selectTypeBadgeName(null, [{ id: "t-1", name: "Bug" }])).toBeNull();
    expect(selectTypeBadgeName(undefined, [{ id: "t-1", name: "Bug" }])).toBeNull();
  });

  it("hides a type the list no longer offers", () => {
    expect(selectTypeBadgeName("t-9", [{ id: "t-1", name: "Bug" }])).toBeNull();
    expect(selectTypeBadgeName("t-1", undefined)).toBeNull();
  });
});

describe("DetailTypeBadgeContent", () => {
  it("renders the issue type name", async () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    mocks.getIssue.mockResolvedValue({ type_id: "t-1" });
    registerEditionIssuesProviders({
      workItemTypes: async () => [{ id: "t-1", name: "Bug" }],
    });
    renderBadge();
    expect(await screen.findByText("Bug")).toBeInTheDocument();
  });

  it("renders nothing for an untyped issue", async () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    mocks.getIssue.mockResolvedValue({ type_id: null });
    registerEditionIssuesProviders({
      workItemTypes: async () => [{ id: "t-1", name: "Bug" }],
    });
    renderBadge();
    await vi.waitFor(() => expect(mocks.getIssue).toHaveBeenCalled());
    expect(screen.queryByText("Bug")).toBeNull();
  });

  it("renders nothing when the type list lacks the id", async () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    mocks.getIssue.mockResolvedValue({ type_id: "t-9" });
    registerEditionIssuesProviders({
      workItemTypes: async () => [{ id: "t-1", name: "Bug" }],
    });
    renderBadge();
    await vi.waitFor(() => expect(mocks.getIssue).toHaveBeenCalled());
    expect(screen.queryByText("Bug")).toBeNull();
  });

  it("renders nothing without the flag", async () => {
    mocks.getIssue.mockResolvedValue({ type_id: "t-1" });
    registerEditionIssuesProviders({
      workItemTypes: async () => [{ id: "t-1", name: "Bug" }],
    });
    renderBadge();
    await vi.waitFor(() => expect(mocks.getIssue).toHaveBeenCalled());
    expect(screen.queryByText("Bug")).toBeNull();
  });

  it("renders nothing without the type source", async () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    mocks.getIssue.mockResolvedValue({ type_id: "t-1" });
    renderBadge();
    await vi.waitFor(() => expect(mocks.getIssue).toHaveBeenCalled());
    expect(screen.queryByText("Bug")).toBeNull();
  });
});
