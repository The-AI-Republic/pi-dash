// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules feature's own project projection (MOD-001/002): it parses a
// record without the switch, so an absent `module_view` reads as off and
// gates the list instead of failing the shared project contract and
// leaving the skeleton up.

import { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import { LIST_STALE_TIME_MS } from "../../../core/query/index.js";
import { makeProjectRow, MODULES_PROJECT_ID } from "../testing.js";
import { moduleKeys } from "./keys.js";
import {
  getModuleProjectFacts,
  ModuleProjectFacts,
  moduleProjectFactsQueryOptions,
  moduleProjectIdentifier,
  moduleProjectTimeZone,
  moduleViewEnabled,
} from "./projectFacts.js";

function client(body: string): ApiClient {
  return {
    getJson: vi.fn(async () => JSON.parse(body) as unknown),
    parse: (schema: { parse(data: unknown): unknown }, data: unknown) => schema.parse(data),
  } as unknown as ApiClient;
}

describe("ModuleProjectFacts", () => {
  it("accepts a full row and keeps unknown fields", () => {
    const facts = ModuleProjectFacts.parse(makeProjectRow({ extra: "kept" }));
    expect(facts.name).toBe("Website relaunch");
    expect(facts.module_view).toBe(true);
    expect(facts["extra"]).toBe("kept");
  });

  it("parses a row with no module_view at all", () => {
    const { module_view: _omitted, ...row } = makeProjectRow();
    const facts = ModuleProjectFacts.parse(row);
    expect(facts.name).toBe("Website relaunch");
    expect(facts.module_view).toBeUndefined();
    expect(moduleViewEnabled(facts.module_view)).toBe(false);
  });

  it("parses null and wrong-typed switch values instead of failing", () => {
    expect(ModuleProjectFacts.parse(makeProjectRow({ module_view: null })).module_view).toBeNull();
    expect(ModuleProjectFacts.parse(makeProjectRow({ module_view: "yes" })).module_view).toBe("yes");
    expect(ModuleProjectFacts.parse(makeProjectRow({ timezone: 5 })).timezone).toBe(5);
  });

  it("still rejects a record without a name", () => {
    expect(() => ModuleProjectFacts.parse({ module_view: true })).toThrow();
  });
});

describe("moduleViewEnabled", () => {
  it("opens only on a literal true", () => {
    expect(moduleViewEnabled(true)).toBe(true);
    expect(moduleViewEnabled(false)).toBe(false);
    expect(moduleViewEnabled(undefined)).toBe(false);
    expect(moduleViewEnabled(null)).toBe(false);
    expect(moduleViewEnabled("true")).toBe(false);
  });
});

describe("moduleProjectTimeZone", () => {
  it("reads a named zone and treats absent or blank as null", () => {
    expect(moduleProjectTimeZone({ name: "P", timezone: "Europe/Berlin" })).toBe("Europe/Berlin");
    expect(moduleProjectTimeZone({ name: "P" })).toBeNull();
    expect(moduleProjectTimeZone({ name: "P", timezone: "  " })).toBeNull();
    expect(moduleProjectTimeZone({ name: "P", timezone: 5 })).toBeNull();
    expect(moduleProjectTimeZone(undefined)).toBeNull();
  });
});

describe("moduleProjectIdentifier", () => {
  it("reads the short identifier and falls back to an empty string", () => {
    expect(moduleProjectIdentifier({ name: "P", identifier: "WEB" } as never)).toBe("WEB");
    expect(moduleProjectIdentifier({ name: "P" } as never)).toBe("");
    expect(moduleProjectIdentifier({ name: "P", identifier: 5 } as never)).toBe("");
    expect(moduleProjectIdentifier(undefined)).toBe("");
  });
});

describe("getModuleProjectFacts", () => {
  it("reads the project row through the modules projection", async () => {
    const api = client(JSON.stringify(makeProjectRow()));
    const facts = await getModuleProjectFacts(api, "acme", MODULES_PROJECT_ID);
    expect(api.getJson).toHaveBeenCalledWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`);
    expect(facts.name).toBe("Website relaunch");
  });
});

describe("moduleProjectFactsQueryOptions", () => {
  it("uses the modules project-facts key and fetch-on-mount freshness (MOD-039)", async () => {
    const api = client(JSON.stringify(makeProjectRow()));
    const options = moduleProjectFactsQueryOptions(api, "acme", MODULES_PROJECT_ID);
    expect(options.queryKey).toEqual(moduleKeys.projectFacts("acme", MODULES_PROJECT_ID));
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);
    const fetched = await new QueryClient().fetchQuery(options);
    expect(fetched.name).toBe("Website relaunch");
  });
});
