// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud issues providers (NEWFRONT-984): the team-project and type
// options read the OSS lists, the bootstrap registers the providers and
// the modal type picker exactly once for a flagged edition and never for
// the community build.
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listIssueTypes: vi.fn(),
  useIssueActivities: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  listProjects: mocks.listProjects,
  listIssueTypes: mocks.listIssueTypes,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

vi.mock("../activity/api.js", () => ({ useIssueActivities: mocks.useIssueActivities }));

import type { Edition } from "../../../core/edition/index.js";
import { getIssueTypeExtension, resetIssueTypeExtension } from "../create/issueTypeSeam.js";
import {
  bootstrapIssuesEdition,
  CloudTransferNote,
  hasIssuesEditionFlags,
  ISSUES_EDITION_FLAGS,
  listTeamProjectOptions,
  listWorkItemTypeOptions,
  resetIssuesEditionBootstrap,
} from "./cloudProviders.js";
import { getEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";

afterEach(() => {
  cleanup();
  resetEditionIssuesProviders();
  resetIssuesEditionBootstrap();
  resetIssueTypeExtension();
  mocks.listProjects.mockReset();
  mocks.listIssueTypes.mockReset();
  mocks.useIssueActivities.mockReset();
});

function editionWith(flags: Record<string, boolean>): Edition {
  return { id: "test", flags };
}

describe("hasIssuesEditionFlags", () => {
  it("is false for the community flags", () => {
    expect(hasIssuesEditionFlags({})).toBe(false);
    expect(hasIssuesEditionFlags({ "plan-badge": true })).toBe(false);
  });

  it("is true when any issues flag is on", () => {
    for (const flag of ISSUES_EDITION_FLAGS) {
      expect(hasIssuesEditionFlags({ [flag]: true })).toBe(true);
    }
    expect(ISSUES_EDITION_FLAGS).toHaveLength(10);
  });
});

describe("listTeamProjectOptions", () => {
  it("maps the workspace project list to filter options", async () => {
    mocks.listProjects.mockResolvedValue([
      { id: "p-1", name: "Alpha", identifier: "ALP" },
      { id: "p-2", name: "Beta", identifier: "BET" },
    ]);
    await expect(listTeamProjectOptions({ workspaceSlug: "acme", projectId: "p-1" })).resolves.toEqual([
      { id: "p-1", name: "Alpha", identifier: "ALP" },
      { id: "p-2", name: "Beta", identifier: "BET" },
    ]);
    expect(mocks.listProjects).toHaveBeenCalledWith({ stub: true }, "acme");
  });
});

describe("listWorkItemTypeOptions", () => {
  it("maps the project type list to picker options", async () => {
    mocks.listIssueTypes.mockResolvedValue([
      { id: "t-1", name: "Bug" },
      { id: "t-2", name: "Epic" },
    ]);
    await expect(listWorkItemTypeOptions({ workspaceSlug: "acme", projectId: "p-1" })).resolves.toEqual([
      { id: "t-1", name: "Bug" },
      { id: "t-2", name: "Epic" },
    ]);
    expect(mocks.listIssueTypes).toHaveBeenCalledWith({ stub: true }, "acme", "p-1");
  });
});

describe("bootstrapIssuesEdition", () => {
  it("registers nothing for the community build", () => {
    bootstrapIssuesEdition(editionWith({}));
    expect(getEditionIssuesProviders()).toEqual({});
    expect(getIssueTypeExtension()).toBeNull();
  });

  it("registers the team projects, transfer note, types and modal picker for a flagged edition", () => {
    bootstrapIssuesEdition(editionWith({ "issue-worklog": true }));
    const providers = getEditionIssuesProviders();
    expect(providers.teamProjects).toBe(listTeamProjectOptions);
    expect(providers.detailBadges?.transferNote).toBe(CloudTransferNote);
    expect(providers.workItemTypes).toBe(listWorkItemTypeOptions);
    expect(getIssueTypeExtension()?.typePicker).toBeDefined();
    expect(getIssueTypeExtension()?.templatePicker).toBeUndefined();
    expect(getIssueTypeExtension()?.additionalProperties).toBeUndefined();
    expect(providers.workflowRules).toBeUndefined();
    expect(providers.issueTypeDetail).toBeUndefined();
  });

  it("runs only once", () => {
    bootstrapIssuesEdition(editionWith({ "issue-worklog": true }));
    resetEditionIssuesProviders();
    bootstrapIssuesEdition(editionWith({ "issue-worklog": true }));
    expect(getEditionIssuesProviders()).toEqual({});
  });
});

describe("CloudTransferNote", () => {
  it("renders the hop sentence once its chunk loads", async () => {
    mocks.useIssueActivities.mockReturnValue({
      data: [
        {
          field: "cycles",
          old_value: "Sprint 3",
          new_value: "Sprint 4",
          old_identifier: "cycle-a",
          new_identifier: "cycle-b",
          created_at: new Date().toISOString(),
        },
      ],
    });
    render(<CloudTransferNote workspaceSlug="acme" projectId="p-1" issueId="i-1" cycleId="cycle-b" />);
    expect(await screen.findByText("Moved from Sprint 3, just now")).toBeInTheDocument();
  });
});
