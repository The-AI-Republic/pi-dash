// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { checkProjectIdentifier, ProjectIdentifierCheck, ProjectIdentifierMatch, updateProject } from "./index.js";
import type { ProjectUpdateInput } from "./index.js";
import {
  checkProjectIdentifier as directCheck,
  ProjectIdentifierCheck as DirectCheck,
  ProjectIdentifierMatch as DirectMatch,
  updateProject as directUpdate,
} from "./contracts/project-management.js";

describe("@pidash/api-client entry", () => {
  it("imports cleanly", async () => {
    const entry = await import("./index");
    expect(entry).toBeDefined();
  });

  it("re-exports the project update and identifier contracts", () => {
    expect(checkProjectIdentifier).toBe(directCheck);
    expect(updateProject).toBe(directUpdate);
    expect(ProjectIdentifierCheck).toBe(DirectCheck);
    expect(ProjectIdentifierMatch).toBe(DirectMatch);
    const input: ProjectUpdateInput = { description: "typed" };
    expect(input.description).toBe("typed");
  });
});
