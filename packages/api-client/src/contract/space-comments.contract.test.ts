// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the public comment thread against live Django (NEWFRONT-752).
// Posts a probe comment, proves the flat reaction shape (NEWFRONT-466),
// edits it, toggles a reaction, deletes it and asserts the thread is clean
// again. Probe rows a crashed run left behind are removed first.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { listSpaceBoardIssues } from "../contracts/space-issues.js";
import {
  addSpaceCommentReaction,
  createSpaceComment,
  deleteSpaceComment,
  listSpaceComments,
  removeSpaceCommentReaction,
  updateSpaceComment,
} from "../contracts/space-comments.js";
import { CONTRACT_BOARD_ANCHOR, contractClient, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE = "Contract space comment probe";

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

async function clearProbes(client: ApiClient, issueId: string): Promise<void> {
  const comments = await listSpaceComments(client, CONTRACT_BOARD_ANCHOR, issueId);
  for (const comment of comments) {
    if (comment.comment_stripped.includes(PROBE)) {
      await deleteSpaceComment(client, CONTRACT_BOARD_ANCHOR, issueId, comment.id);
    }
  }
}

describe.skipIf(!contractEnabled)("space comment contract", () => {
  it("posts, edits, reacts to and deletes a comment", async () => {
    const client = await sharedSignedIn();
    const issueId = await anyIssueId(client);
    await clearProbes(client, issueId);

    const created = await createSpaceComment(client, CONTRACT_BOARD_ANCHOR, issueId, {
      commentHtml: `<p>${PROBE}</p>`,
    });
    expect(created.id).toBeTruthy();
    expect(created.access).toBe("EXTERNAL");
    expect(created.actor_detail?.display_name.length ?? 0).toBeGreaterThan(0);
    expect(created.comment_reactions).toEqual([]);

    const listed = await listSpaceComments(client, CONTRACT_BOARD_ANCHOR, issueId);
    const found = listed.find((comment) => comment.id === created.id);
    expect(found).toBeDefined();
    expect(found?.comment_stripped).toContain(PROBE);

    const edited = await updateSpaceComment(client, CONTRACT_BOARD_ANCHOR, issueId, created.id, {
      commentHtml: `<p>${PROBE} edited</p>`,
    });
    expect(edited.comment_html).toContain("edited");

    const reaction = await addSpaceCommentReaction(client, CONTRACT_BOARD_ANCHOR, created.id, "1f600");
    expect(reaction.reaction).toBe("1f600");
    expect(reaction.comment).toBe(created.id);
    // The public serializer answers the flat actor keys (NEWFRONT-466).
    expect(typeof reaction.display_name).toBe("string");
    expect(reaction).not.toHaveProperty("actor_detail");

    const reacted = await listSpaceComments(client, CONTRACT_BOARD_ANCHOR, issueId);
    const withReaction = reacted.find((comment) => comment.id === created.id);
    expect(withReaction?.comment_reactions.some((row) => row.reaction === "1f600")).toBe(true);

    await removeSpaceCommentReaction(client, CONTRACT_BOARD_ANCHOR, created.id, "1f600");
    await deleteSpaceComment(client, CONTRACT_BOARD_ANCHOR, issueId, created.id);
    const after = await listSpaceComments(client, CONTRACT_BOARD_ANCHOR, issueId);
    expect(after.some((comment) => comment.id === created.id)).toBe(false);
  });

  it("refuses an anonymous comment write", async () => {
    const anonymous = contractClient();
    const issueId = await anyIssueId(anonymous);
    const error = await createSpaceComment(anonymous, CONTRACT_BOARD_ANCHOR, issueId, {
      commentHtml: "<p>nope</p>",
    }).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(401);
  });
});
