// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud issues providers (NEWFRONT-984): the team-project and type
// options read the OSS lists, the bootstrap registers the providers and
// the modal type picker exactly once for a flagged edition and never for
// the community build.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listIssueTypes: vi.fn(),
  getIssue: vi.fn(),
  useIssueActivities: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  listProjects: mocks.listProjects,
  listIssueTypes: mocks.listIssueTypes,
  getIssue: mocks.getIssue,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

vi.mock("../activity/api.js", () => ({ useIssueActivities: mocks.useIssueActivities }));

import { edition, type Edition } from "../../../core/edition/index.js";
import { getIssueTypeExtension, resetIssueTypeExtension } from "../create/issueTypeSeam.js";
import {
  bootstrapIssuesEdition,
  CloudTransferNote,
  CloudTypeBadge,
  CloudTypePicker,
  hasIssuesEditionFlags,
  ISSUES_EDITION_FLAGS,
  listTeamProjectOptions,
  listWorkItemTypeOptions,
  resetIssuesEditionBootstrap,
} from "./cloudProviders.js";
import { ISSUE_TYPES_FLAG } from "./flags.js";
import { getEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";

afterEach(() => {
  cleanup();
  delete edition.flags[ISSUE_TYPES_FLAG];
  resetEditionIssuesProviders();
  resetIssuesEditionBootstrap();
  resetIssueTypeExtension();
  mocks.listProjects.mockReset();
  mocks.listIssueTypes.mockReset();
  mocks.getIssue.mockReset();
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

  it("registers the team projects, transfer note, types, type badge, provisional rules and modal picker for a flagged edition", () => {
    bootstrapIssuesEdition(editionWith({ "issue-worklog": true }));
    const providers = getEditionIssuesProviders();
    expect(providers.teamProjects).toBe(listTeamProjectOptions);
    expect(providers.detailBadges?.transferNote).toBe(CloudTransferNote);
    expect(providers.workItemTypes).toBe(listWorkItemTypeOptions);
    expect(providers.issueTypeDetail?.badge).toBe(CloudTypeBadge);
    expect(providers.issueTypeDetail?.properties).toBeUndefined();
    expect(providers.issueTypeDetail?.activity).toBeUndefined();
    expect(getIssueTypeExtension()?.typePicker).toBe(CloudTypePicker);
    expect(getIssueTypeExtension()?.templatePicker).toBeUndefined();
    expect(getIssueTypeExtension()?.additionalProperties).toBeUndefined();
    expect(providers.workflowRules?.refusal).toBeTypeOf("function");
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

describe("CloudTypeBadge", () => {
  it("renders the type name once its chunk loads", async () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    mocks.getIssue.mockResolvedValue({ type_id: "t-1" });
    bootstrapIssuesEdition(editionWith({ "issue-types": true }));
    mocks.listIssueTypes.mockResolvedValue([{ id: "t-1", name: "Bug" }]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <CloudTypeBadge workspaceSlug="acme" projectId="p-1" issueId="i-1" editable={false} />
      </QueryClientProvider>
    );
    expect(await screen.findByText("Bug")).toBeInTheDocument();
  });
});

describe("CloudTypePicker", () => {
  it("renders the lazy picker and passes the pick through", async () => {
    bootstrapIssuesEdition(editionWith({ "issue-types": true }));
    mocks.listIssueTypes.mockResolvedValue([{ id: "t-1", name: "Bug" }]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const onTypeChange = vi.fn();
    render(
      <QueryClientProvider client={client}>
        <CloudTypePicker
          workspaceSlug="acme"
          projectId="p-1"
          disabled={false}
          typeId={null}
          onTypeChange={onTypeChange}
          onTemplateApply={() => undefined}
          propertyValues={{}}
          onPropertyValuesChange={() => undefined}
        />
      </QueryClientProvider>
    );
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "No type" }));
    await user.click(await screen.findByRole("option", { name: "Bug" }));
    expect(onTypeChange).toHaveBeenCalledWith("t-1");
  });
});
