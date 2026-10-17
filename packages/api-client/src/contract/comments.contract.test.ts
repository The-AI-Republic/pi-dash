// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: issue comments and reactions against live Django. The list
// reads through the history endpoint; writes run against the comments
// collection. Probe rows are deleted by contract, so re-runs start clean.
import { describe, expect, it } from "vitest";
import { listIssues } from "../contracts/issues.js";
import {
  COMMENT_FOLD_LABEL,
  createIssueComment,
  deleteIssueComment,
  listIssueComments,
  updateIssueComment,
} from "../contracts/issue-comments.js";
import {
  createCommentReaction,
  createIssueReaction,
  deleteCommentReaction,
  deleteIssueReaction,
  listCommentReactions,
  listIssueReactions,
} from "../contracts/issue-reactions.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_HTML = "<p>Contract comment probe</p>";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function seededIssueId(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const first = page.results[0];
  expect(first).toBeDefined();
  return first?.id ?? "";
}

/** Remove probe rows a crashed run may have left behind. */
async function clearProbeComments(client: ApiClient, projectId: string, issueId: string): Promise<void> {
  const comments = await listIssueComments(client, CONTRACT_WORKSPACE, projectId, issueId);
  for (const comment of comments) {
    if (comment.comment_html.includes("Contract comment probe")) {
      await deleteIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, comment.id);
    }
  }
}

describe.skipIf(!contractEnabled)("issue comment contract", () => {
  it("creates, lists, edits, toggles access, folds and deletes", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const issueId = await seededIssueId(client, projectId);
    await clearProbeComments(client, projectId, issueId);

    const created = await createIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, {
      comment_html: PROBE_HTML,
      access: "INTERNAL",
    });
    expect(created.id).toBeDefined();
    expect(created.comment_html).toBe(PROBE_HTML);
    expect(created.comment_stripped).toBe("Contract comment probe");
    expect(created.access).toBe("INTERNAL");
    expect(created.edited_at).toBeNull();
    try {
      const listed = await listIssueComments(client, CONTRACT_WORKSPACE, projectId, issueId);
      expect(listed.some((comment) => comment.id === created.id)).toBe(true);

      const edited = await updateIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, created.id, {
        comment_html: "<p>Contract comment probe edited</p>",
      });
      expect(edited.comment_stripped).toBe("Contract comment probe edited");
      expect(edited.edited_at).not.toBeNull();

      const toggled = await updateIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, created.id, {
        access: "EXTERNAL",
      });
      expect(toggled.access).toBe("EXTERNAL");

      const folded = await updateIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, created.id, {
        labels: [COMMENT_FOLD_LABEL],
      });
      expect(folded.labels).toContain(COMMENT_FOLD_LABEL);
      const unfolded = await updateIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, created.id, {
        labels: [],
      });
      expect(unfolded.labels).not.toContain(COMMENT_FOLD_LABEL);
    } finally {
      await deleteIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, created.id);
    }
    const after = await listIssueComments(client, CONTRACT_WORKSPACE, projectId, issueId);
    expect(after.some((comment) => comment.id === created.id)).toBe(false);
  });
});

describe.skipIf(!contractEnabled)("issue reaction contract", () => {
  it("reacts, lists and unreacts on the issue", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const issueId = await seededIssueId(client, projectId);
    // Pre-clean: the server 500s on a duplicate react, so a crashed
    // run's leftover must go before the create below.
    const stale = await listIssueReactions(client, CONTRACT_WORKSPACE, projectId, issueId);
    if (stale.some((row) => row.reaction === "thumbsup")) {
      await deleteIssueReaction(client, CONTRACT_WORKSPACE, projectId, issueId, "thumbsup");
    }
    const created = await createIssueReaction(client, CONTRACT_WORKSPACE, projectId, issueId, "thumbsup");
    expect(created.reaction).toBe("thumbsup");
    expect(created.issue).toBe(issueId);
    try {
      const listed = await listIssueReactions(client, CONTRACT_WORKSPACE, projectId, issueId);
      expect(listed.some((row) => row.id === created.id)).toBe(true);
    } finally {
      await deleteIssueReaction(client, CONTRACT_WORKSPACE, projectId, issueId, "thumbsup");
    }
    const after = await listIssueReactions(client, CONTRACT_WORKSPACE, projectId, issueId);
    expect(after.some((row) => row.reaction === "thumbsup")).toBe(false);
  });

  it("reacts, lists and unreacts on a comment", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const issueId = await seededIssueId(client, projectId);
    await clearProbeComments(client, projectId, issueId);
    const comment = await createIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, {
      comment_html: PROBE_HTML,
    });
    try {
      const created = await createCommentReaction(client, CONTRACT_WORKSPACE, projectId, comment.id, "heart");
      expect(created.reaction).toBe("heart");
      expect(created.comment).toBe(comment.id);
      const listed = await listCommentReactions(client, CONTRACT_WORKSPACE, projectId, comment.id);
      expect(listed.some((row) => row.id === created.id)).toBe(true);
      await deleteCommentReaction(client, CONTRACT_WORKSPACE, projectId, comment.id, "heart");
      const after = await listCommentReactions(client, CONTRACT_WORKSPACE, projectId, comment.id);
      expect(after.some((row) => row.reaction === "heart")).toBe(false);
    } finally {
      await deleteIssueComment(client, CONTRACT_WORKSPACE, projectId, issueId, comment.id);
    }
  });
});
