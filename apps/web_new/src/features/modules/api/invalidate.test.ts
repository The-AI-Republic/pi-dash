// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module freshness (§13, §16): each helper refreshes only the reads the
// write it follows changed, and the post-write helper composes them.

import { QueryClient } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  invalidateAfterModuleWrite,
  invalidateArchivedModuleReads,
  invalidateModuleLists,
  invalidateModuleReads,
  invalidateWorkspaceModules,
} from "./invalidate.js";
import { moduleKeys } from "./keys.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";

function clientSpy(): { client: QueryClient; invalidateQueries: ReturnType<typeof vi.fn> } {
  const invalidateQueries = vi.fn(async () => undefined);
  return { client: { invalidateQueries } as unknown as QueryClient, invalidateQueries };
}

function invalidatedKeys(invalidateQueries: ReturnType<typeof vi.fn>): unknown[] {
  return invalidateQueries.mock.calls.map((call) => (call[0] as { queryKey: unknown }).queryKey);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("invalidateModuleLists", () => {
  it("refreshes the project's list and shares its prefix with the detail", async () => {
    const { client, invalidateQueries } = clientSpy();
    await invalidateModuleLists(client, "acme", PROJECT);
    expect(invalidatedKeys(invalidateQueries)).toEqual([moduleKeys.list("acme", PROJECT)]);
    expect(moduleKeys.list("acme", PROJECT)).not.toEqual(moduleKeys.detail("acme", PROJECT, MODULE));
  });
});

describe("invalidateWorkspaceModules", () => {
  it("refreshes the workspace-wide set", async () => {
    const { client, invalidateQueries } = clientSpy();
    await invalidateWorkspaceModules(client, "acme");
    expect(invalidatedKeys(invalidateQueries)).toEqual([moduleKeys.workspaceList("acme")]);
  });
});

describe("invalidateModuleReads", () => {
  it("refreshes the detail prefix, links and work items with it", async () => {
    const { client, invalidateQueries } = clientSpy();
    await invalidateModuleReads(client, "acme", PROJECT, MODULE);
    expect(invalidatedKeys(invalidateQueries)).toEqual([moduleKeys.detail("acme", PROJECT, MODULE)]);
  });
});

describe("invalidateArchivedModuleReads", () => {
  it("refreshes the archived list prefix", async () => {
    const { client, invalidateQueries } = clientSpy();
    await invalidateArchivedModuleReads(client, "acme", PROJECT);
    expect(invalidatedKeys(invalidateQueries)).toEqual([moduleKeys.archivedList("acme", PROJECT)]);
  });
});

describe("invalidateAfterModuleWrite", () => {
  it("refreshes the active and archived lists plus the touched module", async () => {
    const { client, invalidateQueries } = clientSpy();
    await expect(invalidateAfterModuleWrite(client, "acme", PROJECT, MODULE)).resolves.toBeUndefined();
    expect(invalidatedKeys(invalidateQueries)).toEqual([
      moduleKeys.list("acme", PROJECT),
      moduleKeys.archivedList("acme", PROJECT),
      moduleKeys.detail("acme", PROJECT, MODULE),
    ]);
  });

  it("refreshes only the lists for a create, which has no module id yet", async () => {
    const { client, invalidateQueries } = clientSpy();
    await expect(invalidateAfterModuleWrite(client, "acme", PROJECT, null)).resolves.toBeUndefined();
    expect(invalidatedKeys(invalidateQueries)).toEqual([
      moduleKeys.list("acme", PROJECT),
      moduleKeys.archivedList("acme", PROJECT),
    ]);
  });
});
