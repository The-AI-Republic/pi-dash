// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: space vote and reaction writes against live Django
// (NEWFRONT-752). The board reads engagement from the issue row's
// embedded arrays, so each toggle is proven by re-reading the detail row.
// The suite removes what it adds and leaves the visitor un-voted.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { getSpaceBoardIssue, listSpaceBoardIssues } from "../contracts/space-issues.js";
import {
  addSpaceIssueReaction,
  removeSpaceIssueReaction,
  removeSpaceIssueVote,
  setSpaceIssueVote,
} from "../contracts/space-engagement.js";
import { CONTRACT_BOARD_ANCHOR, CONTRACT_EMAIL, contractClient, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function anyIssueId(client: ApiClient): Promise<string> {
  const page = await listSpaceBoardIssues(client, CONTRACT_BOARD_ANCHOR, {
    groupBy: "state_id",
    perPage: 50,
    cursor: "50:0:0",
  });
  const row = Object.values(page.results).flatMap((bucket) => bucket.results)[0];
  expect(row).toBeDefined();
  return row?.id ?? "";
}

async function clearEngagement(client: ApiClient, issueId: string): Promise<void> {
  await removeSpaceIssueVote(client, CONTRACT_BOARD_ANCHOR, issueId).catch(() => undefined);
  for (const reaction of ["1f600", "1f44d"]) {
    await removeSpaceIssueReaction(client, CONTRACT_BOARD_ANCHOR, issueId, reaction).catch(() => undefined);
  }
}

describe.skipIf(!contractEnabled)("space engagement contract", () => {
  it("sets and removes a vote, reflected on the issue row", async () => {
    const client = await sharedSignedIn();
    const issueId = await anyIssueId(client);
    await clearEngagement(client, issueId);

    const vote = await setSpaceIssueVote(client, CONTRACT_BOARD_ANCHOR, issueId, 1);
    expect(vote.vote).toBe(1);
    expect(vote.actor_detail.display_name.length).toBeGreaterThan(0);
    expect(vote.issue).toBe(issueId);

    const voted = await getSpaceBoardIssue(client, CONTRACT_BOARD_ANCHOR, issueId);
    expect(voted.vote_items.some((item) => item.vote === 1)).toBe(true);

    await setSpaceIssueVote(client, CONTRACT_BOARD_ANCHOR, issueId, -1);
    const downvoted = await getSpaceBoardIssue(client, CONTRACT_BOARD_ANCHOR, issueId);
    expect(downvoted.vote_items.some((item) => item.vote === -1)).toBe(true);
    expect(downvoted.vote_items.some((item) => item.vote === 1)).toBe(false);

    await removeSpaceIssueVote(client, CONTRACT_BOARD_ANCHOR, issueId);
    const cleared = await getSpaceBoardIssue(client, CONTRACT_BOARD_ANCHOR, issueId);
    expect(cleared.vote_items).toEqual([]);
  });

  it("adds and removes an issue reaction, reflected on the issue row", async () => {
    const client = await sharedSignedIn();
    const issueId = await anyIssueId(client);
    await clearEngagement(client, issueId);

    const reaction = await addSpaceIssueReaction(client, CONTRACT_BOARD_ANCHOR, issueId, "1f44d");
    expect(reaction.reaction).toBe("1f44d");
    expect(reaction.issue).toBe(issueId);
    expect(reaction.actor_detail.display_name.length).toBeGreaterThan(0);

    const reacted = await getSpaceBoardIssue(client, CONTRACT_BOARD_ANCHOR, issueId);
    expect(reacted.reaction_items.some((item) => item.reaction === "1f44d")).toBe(true);

    await removeSpaceIssueReaction(client, CONTRACT_BOARD_ANCHOR, issueId, "1f44d");
    const cleared = await getSpaceBoardIssue(client, CONTRACT_BOARD_ANCHOR, issueId);
    expect(cleared.reaction_items.some((item) => item.reaction === "1f44d")).toBe(false);
  });

  it("refuses anonymous engagement writes", async () => {
    const anonymous = contractClient();
    const issueId = await anyIssueId(anonymous);
    const voteError = await setSpaceIssueVote(anonymous, CONTRACT_BOARD_ANCHOR, issueId, 1).catch(
      (caught: unknown) => caught
    );
    expect(voteError).toBeInstanceOf(ApiError);
    expect((voteError as ApiError).status).toBe(401);
    const reactionError = await addSpaceIssueReaction(anonymous, CONTRACT_BOARD_ANCHOR, issueId, "1f600").catch(
      (caught: unknown) => caught
    );
    expect(reactionError).toBeInstanceOf(ApiError);
    expect((reactionError as ApiError).status).toBe(401);
    expect(CONTRACT_EMAIL).toBeTruthy();
  });
});
