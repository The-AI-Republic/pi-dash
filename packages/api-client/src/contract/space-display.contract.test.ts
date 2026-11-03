// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: public display reads against live Django (NEWFRONT-752):
// states, labels, members, cycles and modules for a published board.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  listSpaceBoardCycles,
  listSpaceBoardLabels,
  listSpaceBoardMembers,
  listSpaceBoardModules,
  listSpaceBoardStates,
} from "../contracts/space-display.js";
import { CONTRACT_BOARD_ANCHOR, CONTRACT_EMAIL, contractEnabled, sharedSignedIn } from "./setup.js";

describe.skipIf(!contractEnabled)("space display contract", () => {
  it("lists the board's workflow states", async () => {
    const client = await sharedSignedIn();
    const states = await listSpaceBoardStates(client, CONTRACT_BOARD_ANCHOR);
    expect(states.length).toBeGreaterThan(0);
    for (const state of states) {
      expect(state.name).not.toBe("Triage");
      expect(typeof state.color).toBe("string");
      expect(typeof state.sequence).toBe("number");
    }
    expect(states.map((state) => state.group)).toContain("backlog");
  });

  it("lists labels with a null parent at top level", async () => {
    const client = await sharedSignedIn();
    const labels = await listSpaceBoardLabels(client, CONTRACT_BOARD_ANCHOR);
    expect(labels.length).toBeGreaterThan(0);
    for (const label of labels) {
      expect(typeof label.name).toBe("string");
      expect(typeof label.color).toBe("string");
      expect(label).toHaveProperty("parent");
    }
  });

  it("lists members under the column-name projection", async () => {
    const client = await sharedSignedIn();
    const members = await listSpaceBoardMembers(client, CONTRACT_BOARD_ANCHOR);
    expect(members.length).toBeGreaterThan(0);
    expect(members.map((member) => member.member__display_name)).toContain("contract.tester");
    for (const member of members) {
      expect(typeof member.member__avatar).toBe("string");
    }
  });

  it("lists cycles and modules with their minimal projection", async () => {
    const client = await sharedSignedIn();
    const cycles = await listSpaceBoardCycles(client, CONTRACT_BOARD_ANCHOR);
    expect(cycles.length).toBeGreaterThan(0);
    for (const cycle of cycles) expect(typeof cycle.name).toBe("string");
    const modules = await listSpaceBoardModules(client, CONTRACT_BOARD_ANCHOR);
    expect(modules.length).toBeGreaterThan(0);
    for (const module of modules) expect(typeof module.name).toBe("string");
  });

  it("answers 404 for an anchor that is not published", async () => {
    const readClient = await sharedSignedIn();
    const error = await listSpaceBoardStates(readClient, "no-such-anchor-752").catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    // The seeded address is a member of the board, so this also proves the
    // member projection is not filtered by role.
    const client = await sharedSignedIn();
    const members = await listSpaceBoardMembers(client, CONTRACT_BOARD_ANCHOR);
    expect(members.some((member) => member.member__display_name.length > 0)).toBe(true);
    expect(CONTRACT_EMAIL).toBeTruthy();
  });
});
