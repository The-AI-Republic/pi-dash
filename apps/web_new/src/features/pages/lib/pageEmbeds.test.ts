// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item embed and entity reference producers (PAGE-040/043).

import { describe, expect, it, vi } from "vitest";

import type { EntitySearchIssue, EntitySearchPage } from "@pidash/api-client";

import {
  entityChoiceFromIssue,
  entityChoiceFromPage,
  entityChoiceKey,
  entityReferenceHtml,
  pageEmbedFor,
  pageEntityReferenceFor,
  resolvePastedEntity,
  searchPageEntityChoices,
  type PageEntityChoice,
} from "./pageEmbeds.js";

const issueRow: EntitySearchIssue = {
  name: "Fix login",
  id: "11111111-1111-4111-8111-111111111111",
  sequence_id: 42,
  project__identifier: "WEB",
  project_id: "22222222-2222-4222-8222-222222222222",
  priority: "medium",
  state_id: "33333333-3333-4333-8333-333333333333",
  type_id: null,
};

const pageRow: EntitySearchPage = {
  name: "Launch notes",
  id: "44444444-4444-4444-8444-444444444444",
  logo_props: {},
  projects__id: "55555555-5555-4555-8555-555555555555",
  workspace__slug: "acme",
};

const choice: PageEntityChoice = {
  id: issueRow.id,
  name: issueRow.name,
  sequenceId: issueRow.sequence_id,
  projectId: issueRow.project_id,
  projectIdentifier: issueRow.project__identifier,
  kind: "issue",
  workspaceSlug: "acme",
};

function clientOf(sections: unknown): {
  getJson: ReturnType<typeof vi.fn>;
  parse: (schema: { parse: (value: unknown) => unknown }, value: unknown) => unknown;
} {
  return {
    getJson: vi.fn(async () => sections),
    parse: (schema, value) => schema.parse(value),
  };
}

describe("entity choice mapping", () => {
  it("normalises issue and page search rows", () => {
    expect(entityChoiceFromIssue(issueRow, "acme")).toEqual(choice);
    expect(entityChoiceFromPage(pageRow, "acme")).toEqual({
      id: pageRow.id,
      name: pageRow.name,
      sequenceId: 0,
      projectId: pageRow.projects__id,
      projectIdentifier: "",
      kind: "page",
      workspaceSlug: "acme",
    });
    expect(entityChoiceFromPage({ ...pageRow, projects__id: null }, "acme").projectId).toBe("");
  });

  it("shows an issue key or a page name", () => {
    expect(entityChoiceKey(choice)).toBe("WEB-42");
    expect(entityChoiceKey({ ...choice, kind: "page", name: "Launch notes" })).toBe("Launch notes");
  });
});

describe("stored attributes", () => {
  it("carries every embed identifier", () => {
    expect(pageEmbedFor(choice, "acme")).toEqual({
      id: choice.id,
      entity_identifier: choice.id,
      project_identifier: "WEB",
      workspace_identifier: "acme",
      entity_name: "Fix login",
    });
  });

  it("carries the reference's kind, ids and label", () => {
    expect(pageEntityReferenceFor(choice, "acme")).toEqual({
      entity_name: "issue",
      entity_identifier: choice.id,
      project_identifier: "WEB",
      workspace_identifier: "acme",
      project_id: choice.projectId,
      "data-label": "Fix login",
    });
  });

  it("serializes a reference with escaped attributes", () => {
    const html = entityReferenceHtml({
      entity_name: "issue",
      entity_identifier: 'id"1',
      project_identifier: "P<1",
      workspace_identifier: "acme",
      project_id: "p&1",
      "data-label": "Fix & login",
    });
    expect(html).toContain('entity_identifier="id&quot;1"');
    expect(html).toContain('project_identifier="P&lt;1"');
    expect(html).toContain('project_id="p&amp;1"');
    expect(html).toContain('data-label="Fix &amp; login"');
  });

  it("omits project_id when the reference has none", () => {
    const html = entityReferenceHtml({
      entity_name: "issue",
      entity_identifier: "iss-1",
      project_identifier: "P",
      workspace_identifier: "acme",
      "data-label": "Fix",
    });
    expect(html).not.toContain("project_id=");
  });
});

describe("entity search", () => {
  it("searches project-scoped issues for the picker", async () => {
    const client = clientOf({ issue: [issueRow] });
    const rows = await searchPageEntityChoices(client as never, "acme", issueRow.project_id, "fix");
    expect(rows).toEqual([choice]);
    expect(client.getJson).toHaveBeenCalledWith("/api/workspaces/acme/entity-search/", {
      query: { query: "fix", query_type: "issue", project_id: issueRow.project_id, count: 10 },
    });
  });

  it("includes page rows when asked and tolerates missing sections", async () => {
    const client = clientOf({ issue: [issueRow], page: [pageRow] });
    const rows = await searchPageEntityChoices(client as never, "acme", issueRow.project_id, "", ["issue", "page"]);
    expect(rows.map((row) => row.kind)).toEqual(["issue", "page"]);
    const empty = clientOf({});
    expect(await searchPageEntityChoices(empty as never, "acme", issueRow.project_id, "x")).toEqual([]);
  });

  it("resolves a pasted key only when the search returns that exact row", async () => {
    const client = clientOf({ issue: [issueRow] });
    expect(
      await resolvePastedEntity(client as never, "acme", issueRow.project_id, {
        projectIdentifier: "WEB",
        sequenceId: 42,
      })
    ).toEqual(choice);
    expect(client.getJson).toHaveBeenCalledWith(
      "/api/workspaces/acme/entity-search/",
      expect.objectContaining({
        query: expect.objectContaining({ query: "WEB-42", query_type: "issue", project_id: issueRow.project_id }),
      })
    );
  });

  it("treats a different project code or sequence as unmatched", async () => {
    const client = clientOf({ issue: [issueRow] });
    expect(
      await resolvePastedEntity(client as never, "acme", issueRow.project_id, {
        projectIdentifier: "OTHER",
        sequenceId: 42,
      })
    ).toBeNull();
    expect(
      await resolvePastedEntity(client as never, "acme", issueRow.project_id, {
        projectIdentifier: "WEB",
        sequenceId: 43,
      })
    ).toBeNull();
    const empty = clientOf({});
    expect(
      await resolvePastedEntity(empty as never, "acme", issueRow.project_id, {
        projectIdentifier: "WEB",
        sequenceId: 42,
      })
    ).toBeNull();
  });
});
