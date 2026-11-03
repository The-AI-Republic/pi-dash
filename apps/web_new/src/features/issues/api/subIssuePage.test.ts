// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { createClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { issueKeys } from "./keys.js";
import { subIssuesQueryOptions } from "./subIssuePage.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

describe("row expander children (ISS-082)", () => {
  it("keys the page under the workspace and asks for the first fifty", async () => {
    const seen: string[] = [];
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => {
        seen.push(url);
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () =>
            JSON.stringify({
              sub_issues: [],
              state_distribution: {},
              total_count: 0,
              count: 0,
            }),
        };
      },
      validate: true,
    });
    const options = subIssuesQueryOptions(client, "acme", PROJECT, ISSUE);
    expect(options.queryKey).toEqual([...issueKeys.all("acme"), "sub-issues", PROJECT, ISSUE]);
    expect(options.staleTime).toBeTypeOf("number");
    await new QueryClient().fetchQuery(options);
    expect(seen[0]).toContain(`/issues/${ISSUE}/sub-issues/`);
    expect(seen[0]).toContain("per_page=50");
  });
});
