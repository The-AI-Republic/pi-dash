// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { RecentVisit } from "@pidash/api-client";

import {
  indexProjectIdentifiers,
  indexStateLists,
  issueDetailHref,
  issueKeyLabel,
  pageHref,
  parseRecentRow,
  parseRecentRows,
  projectHref,
  RECENT_FILTER_LABELS,
  RECENT_FILTERS,
  recentProjectIds,
  recentsEntityParam,
  resolveIssueState,
  type RecentIssueData,
} from "./recentRows.js";

function visit(overrides: Partial<RecentVisit> & { entity_name: string }): RecentVisit {
  return {
    id: "visit-1",
    entity_identifier: "entity-1",
    entity_data: null,
    visited_at: "2026-10-14T11:00:00Z",
    ...overrides,
  };
}

const ISSUE_PAYLOAD = {
  id: "issue-1",
  name: "Parity first issue",
  project_id: "project-1",
  sequence_id: 12,
  project_identifier: "ABC",
  state: "state-1",
  priority: "high",
  assignees: ["user-1", "user-2"],
};

describe("recentsEntityParam", () => {
  it("omits the param for All and passes the kind through otherwise", () => {
    expect(recentsEntityParam("all")).toBeUndefined();
    expect(recentsEntityParam("issue")).toBe("issue");
    expect(recentsEntityParam("page")).toBe("page");
    expect(recentsEntityParam("project")).toBe("project");
  });

  it("labels the four filters", () => {
    expect(RECENT_FILTERS.map((filter) => filter.label)).toEqual(["All", "Work items", "Pages", "Projects"]);
    expect(RECENT_FILTER_LABELS.issue).toBe("Work items");
  });
});

describe("parseRecentRow", () => {
  it("skips visits without a payload", () => {
    expect(parseRecentRow(visit({ entity_name: "issue", entity_data: null }))).toBeNull();
  });

  it("skips visits of an unknown kind", () => {
    expect(parseRecentRow(visit({ entity_name: "cycle", entity_data: { id: "c-1", name: "Cycle" } }))).toBeNull();
  });

  it("parses a full work-item payload", () => {
    const row = parseRecentRow(visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD } }));
    expect(row?.visitId).toBe("visit-1");
    expect(row?.visitedAt).toBe("2026-10-14T11:00:00Z");
    expect(row?.entity).toEqual({
      kind: "issue",
      title: "Parity first issue",
      issueId: "issue-1",
      projectId: "project-1",
      sequenceId: 12,
      projectIdentifier: "ABC",
      stateId: "state-1",
      stateNameHint: null,
      priority: "high",
      assigneeCount: 2,
      assigneeNames: [],
    });
  });

  it("falls back to the entity identifier for the title and id", () => {
    const row = parseRecentRow(
      visit({ entity_name: "issue", entity_identifier: "ABC-12", entity_data: { project_id: "project-1" } })
    );
    expect(row?.entity).toMatchObject({ kind: "issue", title: "ABC-12", issueId: "ABC-12" });
  });

  it("treats empty-string fields as missing", () => {
    const row = parseRecentRow(
      visit({
        entity_name: "issue",
        entity_identifier: "ABC-12",
        entity_data: { name: "", project_id: "", project_identifier: "", priority: "" },
      })
    );
    expect(row?.entity).toMatchObject({
      kind: "issue",
      title: "ABC-12",
      projectId: null,
      projectIdentifier: null,
      priority: null,
    });
  });

  it("skips work items with neither a name nor an identifier", () => {
    expect(parseRecentRow(visit({ entity_name: "issue", entity_identifier: "", entity_data: {} }))).toBeNull();
  });

  it("reads the state from state_id and nested state objects", () => {
    const byId = parseRecentRow(
      visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD, state: undefined, state_id: "state-9" } })
    );
    expect(byId?.entity).toMatchObject({ stateId: "state-9", stateNameHint: null });
    const nested = parseRecentRow(
      visit({
        entity_name: "issue",
        entity_data: { ...ISSUE_PAYLOAD, state: { id: "state-2", name: "In progress" } },
      })
    );
    expect(nested?.entity).toMatchObject({ stateId: "state-2", stateNameHint: "In progress" });
    const bare = parseRecentRow(visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD, state: undefined } }));
    expect(bare?.entity).toMatchObject({ stateId: null, stateNameHint: null });
  });

  it("treats an empty state string as missing", () => {
    const row = parseRecentRow(visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD, state: "" } }));
    expect(row?.entity).toMatchObject({ stateId: null });
  });

  it("reads assignee names from nested member objects", () => {
    const row = parseRecentRow(
      visit({
        entity_name: "issue",
        entity_data: {
          ...ISSUE_PAYLOAD,
          assignees: [
            { display_name: "Ada Lovelace" },
            { name: "Grace Hopper" },
            { display_name: "" },
            { id: "user-9" },
            "user-10",
            null,
          ],
        },
      })
    );
    expect(row?.entity).toMatchObject({
      assigneeCount: 6,
      assigneeNames: ["Ada Lovelace", "Grace Hopper"],
    });
  });

  it("reads legacy assignee_ids and non-array assignees", () => {
    const legacy = parseRecentRow(
      visit({
        entity_name: "issue",
        entity_data: { ...ISSUE_PAYLOAD, assignees: undefined, assignee_ids: ["user-3"] },
      })
    );
    expect(legacy?.entity).toMatchObject({ assigneeCount: 1, assigneeNames: [] });
    const broken = parseRecentRow(
      visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD, assignees: "user-3" } })
    );
    expect(broken?.entity).toMatchObject({ assigneeCount: 0, assigneeNames: [] });
  });

  it("drops misshapen scalar fields instead of breaking", () => {
    for (const sequence_id of [Number.NaN, "12", null]) {
      const row = parseRecentRow(
        visit({
          entity_name: "issue",
          entity_data: {
            name: "Odd issue",
            id: 42,
            project_id: 7,
            sequence_id,
            project_identifier: false,
            priority: ["high"],
            state: 12,
          },
        })
      );
      expect(row?.entity).toMatchObject({
        kind: "issue",
        title: "Odd issue",
        projectId: null,
        sequenceId: null,
        projectIdentifier: null,
        stateId: null,
        priority: null,
      });
    }
  });

  it("parses page and workspace_page payloads alike", () => {
    for (const entity_name of ["page", "workspace_page"]) {
      const row = parseRecentRow(
        visit({
          entity_name,
          entity_data: {
            id: "page-1",
            name: "Launch notes",
            project_id: "project-1",
            project_identifier: "ABC",
            owned_by: "user-1",
          },
        })
      );
      expect(row?.entity).toEqual({
        kind: "page",
        title: "Launch notes",
        pageId: "page-1",
        projectId: "project-1",
        projectIdentifier: "ABC",
        hasOwner: true,
      });
    }
  });

  it("degrades page rows with missing fields", () => {
    const row = parseRecentRow(
      visit({ entity_name: "page", entity_identifier: "page-9", entity_data: { project_id: null } })
    );
    expect(row?.entity).toMatchObject({
      kind: "page",
      title: "page-9",
      pageId: "page-9",
      projectId: null,
      hasOwner: false,
    });
    expect(parseRecentRow(visit({ entity_name: "page", entity_identifier: "", entity_data: {} }))).toBeNull();
  });

  it("parses project payloads with member counts", () => {
    const row = parseRecentRow(
      visit({
        entity_name: "project",
        entity_data: { id: "project-1", name: "Website", identifier: "WEB", project_members: ["a", "b"] },
      })
    );
    expect(row?.entity).toEqual({
      kind: "project",
      title: "Website",
      projectId: "project-1",
      identifier: "WEB",
      memberCount: 2,
    });
  });

  it("degrades project rows with missing fields", () => {
    const row = parseRecentRow(
      visit({ entity_name: "project", entity_identifier: "project-9", entity_data: { name: "Solo" } })
    );
    expect(row?.entity).toMatchObject({
      kind: "project",
      title: "Solo",
      projectId: "project-9",
      identifier: null,
      memberCount: 0,
    });
    expect(parseRecentRow(visit({ entity_name: "project", entity_identifier: "", entity_data: {} }))).toBeNull();
  });
});

describe("parseRecentRows", () => {
  it("keeps visit order and drops unrenderable visits", () => {
    const rows = parseRecentRows([
      visit({ id: "v-1", entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD } }),
      visit({ id: "v-2", entity_name: "issue", entity_data: null }),
      visit({ id: "v-3", entity_name: "cycle", entity_data: { id: "c", name: "Cycle" } }),
      visit({
        id: "v-4",
        entity_name: "project",
        entity_data: { id: "project-1", name: "Website", identifier: "WEB" },
      }),
    ]);
    expect(rows.map((row) => row.visitId)).toEqual(["v-1", "v-4"]);
  });
});

describe("issueKeyLabel", () => {
  const issue = (overrides: Partial<RecentIssueData>): RecentIssueData => ({
    kind: "issue",
    title: "Issue",
    issueId: "issue-1",
    projectId: "project-1",
    sequenceId: 12,
    projectIdentifier: "ABC",
    stateId: null,
    stateNameHint: null,
    priority: null,
    assigneeCount: 0,
    assigneeNames: [],
    ...overrides,
  });

  it("combines the identifier with the sequence number", () => {
    expect(issueKeyLabel(issue({}), null)).toBe("ABC-12");
  });

  it("falls back to the project list identifier", () => {
    expect(issueKeyLabel(issue({ projectIdentifier: null }), "WEB")).toBe("WEB-12");
  });

  it("shows the lone identifier without a sequence number", () => {
    expect(issueKeyLabel(issue({ sequenceId: null }), null)).toBe("ABC");
  });

  it("resolves to null with no identifier anywhere", () => {
    expect(issueKeyLabel(issue({ projectIdentifier: null, sequenceId: null }), null)).toBeNull();
  });
});

describe("href builders", () => {
  it("points work items at their detail view, or home without a project", () => {
    expect(issueDetailHref("acme", "project-1", "issue-1")).toBe("/acme/projects/project-1/issues/issue-1");
    expect(issueDetailHref("acme", null, "issue-1")).toBe("/acme/");
  });

  it("scopes page addresses to the project when bound", () => {
    expect(pageHref("acme", "project-1", "page-1")).toBe("/acme/projects/project-1/pages/page-1");
    expect(pageHref("acme", null, "page-1")).toBe("/acme/pages/page-1");
  });

  it("points projects at their work list", () => {
    expect(projectHref("acme", "project-1")).toBe("/acme/projects/project-1/issues");
  });
});

describe("recentProjectIds", () => {
  it("collects distinct project ids from issue rows only", () => {
    const rows = parseRecentRows([
      visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD, project_id: "p-1" } }),
      visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD, project_id: "p-1" } }),
      visit({ entity_name: "issue", entity_data: { ...ISSUE_PAYLOAD, project_id: "p-2" } }),
      visit({ entity_name: "issue", entity_data: { name: "Orphan", project_id: null } }),
      visit({ entity_name: "page", entity_data: { id: "g-1", name: "Notes", project_id: "p-9" } }),
    ]);
    expect(recentProjectIds(rows)).toEqual(["p-1", "p-2"]);
  });
});

describe("indexStateLists", () => {
  it("indexes reads by id and skips pending entries", () => {
    const byId = indexStateLists([
      undefined,
      [
        {
          id: "state-1",
          project_id: "p-1",
          workspace_id: "w-1",
          name: "In progress",
          color: "#ff0000",
          group: "started",
          default: false,
          description: "",
          sequence: 1,
        },
      ],
    ]);
    expect(byId.get("state-1")).toEqual({ name: "In progress", color: "#ff0000" });
    expect(byId.get("missing")).toBeUndefined();
  });
});

describe("resolveIssueState", () => {
  const issue = (overrides: Partial<RecentIssueData>): RecentIssueData => ({
    kind: "issue",
    title: "Issue",
    issueId: "issue-1",
    projectId: "project-1",
    sequenceId: null,
    projectIdentifier: null,
    stateId: "state-1",
    stateNameHint: null,
    priority: null,
    assigneeCount: 0,
    assigneeNames: [],
    ...overrides,
  });
  const statesById = new Map([["state-1", { name: "In progress", color: "#ff0000" }]]);

  it("prefers the nested state hint", () => {
    expect(resolveIssueState(issue({ stateNameHint: "Review" }), statesById)).toEqual({
      name: "Review",
      color: null,
    });
  });

  it("resolves the id through the states lookup", () => {
    expect(resolveIssueState(issue({}), statesById)).toEqual({ name: "In progress", color: "#ff0000" });
  });

  it("degrades to null without an id or a lookup hit", () => {
    expect(resolveIssueState(issue({ stateId: null }), statesById)).toEqual({ name: null, color: null });
    expect(resolveIssueState(issue({ stateId: "missing" }), statesById)).toEqual({
      name: null,
      color: null,
    });
  });
});

describe("indexProjectIdentifiers", () => {
  it("indexes the list by id and tolerates a missing list", () => {
    expect(indexProjectIdentifiers(undefined).size).toBe(0);
    const byId = indexProjectIdentifiers([
      {
        id: "project-1",
        name: "Website",
        identifier: "WEB",
        sort_order: null,
        logo_props: {},
        member_role: null,
        intake_count: 0,
        archived_at: null,
        workspace: "w-1",
        cycle_view: false,
        issue_views_view: true,
        module_view: false,
        page_view: false,
        inbox_view: false,
        is_default: false,
        guest_view_all_features: false,
        project_lead: null,
        network: 0,
        created_at: "2026-10-01T00:00:00Z",
        updated_at: "2026-10-01T00:00:00Z",
        created_by: null,
        updated_by: null,
      },
    ]);
    expect(byId.get("project-1")).toBe("WEB");
  });
});
