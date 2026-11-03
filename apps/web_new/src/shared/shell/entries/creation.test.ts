// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette creation entries (shell spec, SHELL-086/092).
import { describe, expect, it, vi } from "vitest";
import { buildCreationCommands } from "./creation.js";

describe("creation entries", () => {
  it("offers the project dialog and the workspace page when allowed", () => {
    const onOpenCreateProject = vi.fn();
    const onCreateWorkspace = vi.fn();
    const commands = buildCreationCommands({
      canCreateProject: true,
      canCreateWorkspace: true,
      onOpenCreateProject,
      onCreateWorkspace,
    });
    expect(commands.map((command) => [command.id, command.title, command.section, command.sequence])).toEqual([
      ["shell.create-project", "New project", "Create", "np"],
      ["shell.create-workspace", "New workspace", "Create", undefined],
    ]);
    commands[0]!.run();
    commands[1]!.run();
    expect(onOpenCreateProject).toHaveBeenCalledTimes(1);
    expect(onCreateWorkspace).toHaveBeenCalledTimes(1);
  });

  it("hides the project entry without create rights, keeping the workspace one", () => {
    const commands = buildCreationCommands({
      canCreateProject: false,
      canCreateWorkspace: true,
      onOpenCreateProject: () => undefined,
      onCreateWorkspace: () => undefined,
    });
    expect(commands.map((command) => command.id)).toEqual(["shell.create-workspace"]);
  });

  it("hides the workspace entry when the instance disables creation", () => {
    const commands = buildCreationCommands({
      canCreateProject: true,
      canCreateWorkspace: false,
      onOpenCreateProject: () => undefined,
      onCreateWorkspace: () => undefined,
    });
    expect(commands.map((command) => command.id)).toEqual(["shell.create-project"]);
  });

  it("hides everything when nothing is allowed", () => {
    expect(
      buildCreationCommands({
        canCreateProject: false,
        canCreateWorkspace: false,
        onOpenCreateProject: () => undefined,
        onCreateWorkspace: () => undefined,
      })
    ).toEqual([]);
  });
});
