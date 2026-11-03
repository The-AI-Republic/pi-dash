// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules palette commands (parity/specs/modules.md §10.2, MOD-037): the
// creation entry exists only inside a project whose modules switch is on
// and whose viewer is a member or admin, carries the pinned title and the
// two-key `nm` sequence; the jump entry exists whenever the switch is on
// and opens the picker page.
import { describe, expect, it, vi } from "vitest";

import {
  buildModuleCreateCommand,
  buildModuleJumpCommand,
  MODULE_CREATE_COMMAND_ID,
  MODULE_CREATE_SEQUENCE,
  MODULE_CREATE_TITLE,
  MODULE_JUMP_COMMAND_ID,
  MODULE_JUMP_PAGE_ID,
  MODULE_JUMP_TITLE,
} from "./moduleCommands.js";

const PROJECT_NAME = "Website relaunch";

describe("buildModuleCreateCommand", () => {
  it("builds the pinned creation entry for a member in an enabled project", () => {
    const onCreate = vi.fn();
    const command = buildModuleCreateCommand({
      projectName: PROJECT_NAME,
      modulesEnabled: true,
      canCreate: true,
      onCreate,
    });
    expect(command).toMatchObject({
      id: MODULE_CREATE_COMMAND_ID,
      title: "New module",
      hint: PROJECT_NAME,
      section: "Create",
      sequence: "nm",
    });
    expect(MODULE_CREATE_TITLE).toBe("New module");
    expect(MODULE_CREATE_SEQUENCE).toBe("nm");
    expect(command?.keywords).toEqual(expect.arrayContaining(["module", "create"]));
    expect(command?.page).toBeUndefined();
    command?.run();
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("registers nothing while the modules switch is off", () => {
    const command = buildModuleCreateCommand({
      projectName: PROJECT_NAME,
      modulesEnabled: false,
      canCreate: true,
      onCreate: vi.fn(),
    });
    expect(command).toBeNull();
  });

  it("registers nothing for a project guest", () => {
    const command = buildModuleCreateCommand({
      projectName: PROJECT_NAME,
      modulesEnabled: true,
      canCreate: false,
      onCreate: vi.fn(),
    });
    expect(command).toBeNull();
  });
});

describe("buildModuleJumpCommand", () => {
  it("opens the module picker page in an enabled project", () => {
    const command = buildModuleJumpCommand({ projectName: PROJECT_NAME, modulesEnabled: true });
    expect(command).toMatchObject({
      id: MODULE_JUMP_COMMAND_ID,
      title: "Open a module",
      hint: PROJECT_NAME,
      section: "Navigate",
      page: MODULE_JUMP_PAGE_ID,
    });
    expect(MODULE_JUMP_TITLE).toBe("Open a module");
    expect(command?.sequence).toBeUndefined();
    expect(command?.run()).toBeUndefined();
  });

  it("registers nothing while the modules switch is off", () => {
    expect(buildModuleJumpCommand({ projectName: PROJECT_NAME, modulesEnabled: false })).toBeNull();
  });
});
