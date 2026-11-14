// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles palette command (parity/specs/cycles.md §6, CYC-014): the
// creation entry exists only inside a project whose cycles switch is on
// and whose viewer is a member or admin, and carries the pinned title
// and the two-key `nc` sequence.
import { describe, expect, it, vi } from "vitest";

import {
  buildCycleCreateCommand,
  CYCLE_CREATE_COMMAND_ID,
  CYCLE_CREATE_SEQUENCE,
  CYCLE_CREATE_TITLE,
} from "./cycleCommands.js";

const PROJECT_NAME = "Website relaunch";

describe("buildCycleCreateCommand", () => {
  it("builds the pinned creation entry for a member in an enabled project", () => {
    const onCreate = vi.fn();
    const command = buildCycleCreateCommand({
      projectName: PROJECT_NAME,
      cyclesEnabled: true,
      canCreate: true,
      onCreate,
    });
    expect(command).toMatchObject({
      id: CYCLE_CREATE_COMMAND_ID,
      title: "New cycle",
      hint: PROJECT_NAME,
      section: "Create",
      sequence: "nc",
    });
    expect(CYCLE_CREATE_TITLE).toBe("New cycle");
    expect(CYCLE_CREATE_SEQUENCE).toBe("nc");
    expect(command?.keywords).toEqual(expect.arrayContaining(["cycle", "create"]));
    expect(command?.page).toBeUndefined();
    command?.run();
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("registers nothing while the cycles switch is off", () => {
    const command = buildCycleCreateCommand({
      projectName: PROJECT_NAME,
      cyclesEnabled: false,
      canCreate: true,
      onCreate: vi.fn(),
    });
    expect(command).toBeNull();
  });

  it("registers nothing for a project guest", () => {
    const command = buildCycleCreateCommand({
      projectName: PROJECT_NAME,
      cyclesEnabled: true,
      canCreate: false,
      onCreate: vi.fn(),
    });
    expect(command).toBeNull();
  });

  it("registers nothing when both gates fail", () => {
    const command = buildCycleCreateCommand({
      projectName: PROJECT_NAME,
      cyclesEnabled: false,
      canCreate: false,
      onCreate: vi.fn(),
    });
    expect(command).toBeNull();
  });
});
