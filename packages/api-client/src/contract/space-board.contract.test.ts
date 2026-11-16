// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: public board resolution against live Django (NEWFRONT-752).
// The seed publishes the contract board at a fixed anchor; these suites
// read it, resolve the legacy workspace/project address to the same
// anchor, and pin the 404 for an unpublished one.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { getSpaceBoardMeta, getSpaceBoardSettings, resolveSpaceBoardAnchor } from "../contracts/space-board.js";
import {
  CONTRACT_BOARD_ANCHOR,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_WORKSPACE,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";

describe.skipIf(!contractEnabled)("space board contract", () => {
  it("reads the published board's settings payload", async () => {
    const client = await sharedSignedIn();
    const board = await getSpaceBoardSettings(client, CONTRACT_BOARD_ANCHOR);
    expect(board.anchor).toBe(CONTRACT_BOARD_ANCHOR);
    expect(board.entity_name).toBe("project");
    expect(board.project_details?.identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
    expect(board.workspace_detail.slug).toBe(CONTRACT_WORKSPACE);
    expect(board.is_comments_enabled).toBe(true);
    expect(board.is_reactions_enabled).toBe(true);
    expect(board.is_votes_enabled).toBe(true);
    expect(board.view_props["list"]).toBe(true);
    expect(board.view_props["kanban"]).toBe(true);
  });

  it("resolves the legacy workspace/project address to the anchor", async () => {
    const client = await sharedSignedIn();
    const board = await getSpaceBoardSettings(client, CONTRACT_BOARD_ANCHOR);
    const projectId = board.project_details?.id ?? "";
    expect(projectId).not.toBe("");
    const resolved = await resolveSpaceBoardAnchor(client, CONTRACT_WORKSPACE, projectId);
    expect(resolved.anchor).toBe(CONTRACT_BOARD_ANCHOR);
    expect(resolved.project_details?.identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
  });

  it("reads board metadata without exposing a cover image", async () => {
    const meta = await getSpaceBoardMeta(await sharedSignedIn(), CONTRACT_BOARD_ANCHOR);
    expect(meta.identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
    expect(typeof meta.name).toBe("string");
  });

  it("answers 404 for an anchor that is not published", async () => {
    const error = await getSpaceBoardSettings(await sharedSignedIn(), "no-such-anchor-752").catch(
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
  });

  it("answers 404 for a legacy address whose project has no board", async () => {
    const error = await resolveSpaceBoardAnchor(
      await sharedSignedIn(),
      CONTRACT_WORKSPACE,
      "123e4567-e89b-12d3-a456-426614174000"
    ).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
  });
});
