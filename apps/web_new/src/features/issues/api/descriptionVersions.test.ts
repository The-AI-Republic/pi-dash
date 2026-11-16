// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { createClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { descriptionVersionQueryOptions, descriptionVersionsQueryOptions } from "./descriptionVersions.js";
import { issueKeys } from "./keys.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const VERSION = "323e4567-e89b-12d3-a456-426614174002";

const VERSION_ROW = {
  id: VERSION,
  workspace: "423e4567-e89b-12d3-a456-426614174003",
  project: PROJECT,
  issue: ISSUE,
  last_saved_at: "2024-02-03T04:05:06Z",
  owned_by: "523e4567-e89b-12d3-a456-426614174004",
  created_at: "2024-02-03T04:05:06Z",
  updated_at: "2024-02-03T04:05:06Z",
  created_by: null,
  updated_by: null,
};

const VERSIONS_ENVELOPE = {
  prev_cursor: "",
  cursor: "",
  next_cursor: null,
  prev_page_results: false,
  next_page_results: false,
  page_count: 1,
  total_results: 1,
  total_pages: 1,
  results: [VERSION_ROW],
};

const VERSION_DETAIL = {
  ...VERSION_ROW,
  description_binary: null,
  description_html: "<p>restored</p>",
  description_stripped: "restored",
  description_json: {},
};

describe("description version queries (ISS-147)", () => {
  it("keys the list and one version per work item", () => {
    const client = createClient({ baseUrl: "https://api.test", transport: async () => never() });
    expect(descriptionVersionsQueryOptions(client, "acme", PROJECT, ISSUE).queryKey).toEqual(
      issueKeys.descriptionVersions("acme", PROJECT, ISSUE)
    );
    expect(descriptionVersionQueryOptions(client, "acme", PROJECT, ISSUE, VERSION).queryKey).toEqual(
      issueKeys.descriptionVersion("acme", PROJECT, ISSUE, VERSION)
    );
  });

  it("reads the version history and one version through the work-item URLs", async () => {
    const seen: string[] = [];
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => {
        seen.push(url);
        const body = url.includes(`description-versions/${VERSION.slice(0, 8)}`) ? VERSION_DETAIL : VERSIONS_ENVELOPE;
        return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
      },
      validate: true,
    });
    const queryClient = new QueryClient();
    const list = await queryClient.fetchQuery(descriptionVersionsQueryOptions(client, "acme", PROJECT, ISSUE));
    expect(list.results).toHaveLength(1);
    const one = await queryClient.fetchQuery(descriptionVersionQueryOptions(client, "acme", PROJECT, ISSUE, VERSION));
    expect(one.description_html).toBe("<p>restored</p>");
    expect(seen[0]).toContain("/description-versions/");
    expect(seen[1]).toContain(`/description-versions/${VERSION}/`);
  });
});

function never(): Promise<never> {
  return Promise.reject(new Error("a query key read makes no request"));
}
