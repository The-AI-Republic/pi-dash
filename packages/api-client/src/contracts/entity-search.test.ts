// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stub proofs for the workspace entity search (NEWFRONT-833, spec §11.5;
// PAGE-041/043): the preview read types the sections a page body draws
// cards for and ignores a kind it did not ask for, the mention helper keeps
// only people, and the base envelope call is untouched.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  ENTITY_PREVIEW_KINDS,
  searchEntityPreviews,
  searchEntitySections,
  searchMentionPeople,
} from "./entity-search.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PATH = `/api/workspaces/${WS}/entity-search/`;

interface Call {
  url: string;
  query?: unknown;
}

function stubClient(body: unknown): { client: ApiClient; calls: Call[] } {
  const calls: Call[] = [];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: (async (url: string, options: { query?: unknown } = {}) => {
      calls.push({ url, query: options.query });
      return body;
    }) as ApiClient["getJson"],
  } as ApiClient;
  return { client, calls };
}

const PEOPLE = [{ member__id: "4", member__display_name: "Ada", member__avatar_url: null }];

describe("entity preview contract", () => {
  it("asks for every preview kind unless the caller narrows it", async () => {
    const { client, calls } = stubClient({ page: [] });
    await searchEntityPreviews(client, WS, { query: "ro", projectId: PROJECT, count: 5 });
    expect(calls[0]?.url).toBe(PATH);
    expect(calls[0]?.query).toEqual({
      query: "ro",
      query_type: ENTITY_PREVIEW_KINDS.join(","),
      project_id: PROJECT,
      count: 5,
    });
    expect(ENTITY_PREVIEW_KINDS).not.toContain("user_mention");
  });

  it("parses one typed section per asked kind and ignores absent ones", async () => {
    const { client, calls } = stubClient({
      page: [{ name: "Roadmap", id: PROJECT, logo_props: {}, projects__id: PROJECT, workspace__slug: WS }],
      issue: [
        {
          name: "Ship it",
          id: PROJECT,
          sequence_id: 12,
          project__identifier: "CT",
          project_id: PROJECT,
          priority: "medium",
          state_id: PROJECT,
          type_id: null,
        },
      ],
    });
    const result = await searchEntityPreviews(client, WS, { query: "ro", kinds: ["page", "issue"] });
    expect(calls[0]?.query).toMatchObject({ query_type: "page,issue" });
    expect(result.page?.[0]?.name).toBe("Roadmap");
    expect(result.page?.[0]?.projects__id).toBe(PROJECT);
    expect(result.issue?.[0]?.sequence_id).toBe(12);
    expect(result.project).toBeUndefined();
    expect(result.cycle).toBeUndefined();
  });

  it("parses every preview kind the endpoint knows", async () => {
    const id = PROJECT;
    const { client } = stubClient({
      project: [{ name: "P", id, identifier: "CT", logo_props: {}, workspace__slug: WS }],
      issue: [
        {
          name: "I",
          id,
          sequence_id: 1,
          project__identifier: "CT",
          project_id: id,
          priority: "low",
          state_id: id,
          type_id: id,
        },
      ],
      cycle: [{ name: "C", id, project_id: id, project__identifier: "CT", status: "current", workspace__slug: WS }],
      module: [{ name: "M", id, project_id: id, project__identifier: "CT", status: "planned", workspace__slug: WS }],
      page: [{ name: "Page", id, logo_props: {}, projects__id: id, workspace__slug: WS }],
    });
    const result = await searchEntityPreviews(client, WS, { query: "x" });
    expect(result.project?.[0]?.identifier).toBe("CT");
    expect(result.issue?.[0]?.type_id).toBe(id);
    expect(result.cycle?.[0]?.status).toBe("current");
    expect(result.module?.[0]?.status).toBe("planned");
    expect(result.page?.[0]?.name).toBe("Page");
  });

  it("ignores a section the caller did not ask for", async () => {
    const { client } = stubClient({
      page: [{ name: "Roadmap", id: PROJECT, logo_props: {}, projects__id: PROJECT, workspace__slug: WS }],
      issue: [{ malformed: true }],
    });
    const result = await searchEntityPreviews(client, WS, { query: "ro", kinds: ["page"] });
    expect(result.page?.[0]?.name).toBe("Roadmap");
    expect(result.issue).toBeUndefined();
  });

  it("rejects a section whose row lost its id", async () => {
    const { client } = stubClient({ page: [{ name: "Roadmap" }] });
    await expect(searchEntityPreviews(client, WS, { query: "ro", kinds: ["page"] })).rejects.toThrow();
  });

  it("defaults the untyped envelope read to the people section", async () => {
    const { client, calls } = stubClient({ user_mention: PEOPLE });
    const sections = await searchEntitySections(client, WS, { query: "ad", projectId: PROJECT });
    expect(calls[0]?.query).toMatchObject({ query_type: "user_mention", project_id: PROJECT });
    expect(sections.user_mention).toEqual(PEOPLE);
  });

  it("leaves the untyped envelope read additive as before", async () => {
    const { client, calls } = stubClient({ user_mention: PEOPLE, page: [{ anything: true }] });
    const sections = await searchEntitySections(client, WS, { query: "ad", queryTypes: ["user_mention", "page"] });
    expect(calls[0]?.query).toMatchObject({ query_type: "user_mention,page" });
    expect(sections.user_mention).toEqual(PEOPLE);
    expect(sections.page).toEqual([{ anything: true }]);
  });

  it("keeps only people for the mention helper", async () => {
    const { client, calls } = stubClient({ user_mention: PEOPLE, page: [] });
    const people = await searchMentionPeople(client, WS, { query: "ad", projectId: PROJECT, count: 4 });
    expect((calls[0]?.query as { query_type: string } | undefined)?.query_type).toBe("user_mention");
    expect(people).toEqual(PEOPLE);
  });

  it("answers an empty list when the server omits the people section", async () => {
    const { client } = stubClient({ page: [] });
    await expect(searchMentionPeople(client, WS, { query: "zz" })).resolves.toEqual([]);
  });
});
