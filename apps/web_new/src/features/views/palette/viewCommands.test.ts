// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import {
  buildViewCreateCommand,
  buildViewJumpCommand,
  VIEW_CREATE_COMMAND_ID,
  VIEW_CREATE_SEQUENCE,
  VIEW_CREATE_TITLE,
  VIEW_JUMP_COMMAND_ID,
  VIEW_JUMP_PAGE_ID,
  VIEW_JUMP_SEQUENCE,
  VIEW_JUMP_TITLE,
} from "./viewCommands.js";

describe("buildViewCreateCommand", () => {
  it("registers the gated creation entry for a member with the switch on", () => {
    const onCreateView = vi.fn();
    const command = buildViewCreateCommand({
      projectName: "Website relaunch",
      viewsEnabled: true,
      canCreateView: true,
      onCreateView,
    });
    expect(command).toEqual({
      id: VIEW_CREATE_COMMAND_ID,
      title: VIEW_CREATE_TITLE,
      hint: "Website relaunch",
      section: "Create",
      sequence: VIEW_CREATE_SEQUENCE,
      keywords: ["create", "add", "new", "view"],
      run: onCreateView,
    });
    expect(VIEW_CREATE_TITLE).toBe("New view");
    expect(VIEW_CREATE_SEQUENCE).toBe("nv");
  });

  it("stays absent while the switch is off", () => {
    expect(
      buildViewCreateCommand({
        projectName: "Website relaunch",
        viewsEnabled: false,
        canCreateView: true,
        onCreateView: () => undefined,
      })
    ).toBeNull();
  });

  it("stays absent for a guest with the switch on", () => {
    expect(
      buildViewCreateCommand({
        projectName: "Website relaunch",
        viewsEnabled: true,
        canCreateView: false,
        onCreateView: () => undefined,
      })
    ).toBeNull();
  });

  it("stays absent when both gates fail", () => {
    expect(
      buildViewCreateCommand({
        projectName: "Website relaunch",
        viewsEnabled: false,
        canCreateView: false,
        onCreateView: () => undefined,
      })
    ).toBeNull();
  });
});

describe("buildViewJumpCommand", () => {
  it("registers the jump entry with its sub-page while the switch is on", () => {
    const command = buildViewJumpCommand({ projectName: "Website relaunch", viewsEnabled: true });
    expect(command).toMatchObject({
      id: VIEW_JUMP_COMMAND_ID,
      title: VIEW_JUMP_TITLE,
      hint: "Website relaunch",
      section: "Navigate",
      sequence: VIEW_JUMP_SEQUENCE,
      page: VIEW_JUMP_PAGE_ID,
    });
    expect(VIEW_JUMP_TITLE).toBe("Open a project view");
    expect(VIEW_JUMP_SEQUENCE).toBe("ov");
    expect(command?.run()).toBeUndefined();
  });

  it("stays absent while the switch is off", () => {
    expect(buildViewJumpCommand({ projectName: "Website relaunch", viewsEnabled: false })).toBeNull();
  });
});
