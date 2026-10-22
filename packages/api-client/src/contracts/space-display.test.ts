// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the public display reads: states, labels, members, cycles
// and modules, all anchor-scoped.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import {
  listSpaceBoardCycles,
  listSpaceBoardLabels,
  listSpaceBoardMembers,
  listSpaceBoardModules,
  listSpaceBoardStates,
} from "./space-display.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function clientFor(value: unknown) {
  const transport = recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse({ body: JSON.stringify(value) })
  );
  return { client: createClient({ baseUrl: "https://api.test", transport }), transport };
}

describe("space display contracts", () => {
  it("lists states with name, group, color, id and sequence", async () => {
    const { client, transport } = clientFor([
      { name: "Backlog", group: "backlog", color: "#60646C", id: UUID, sequence: 15000 },
    ]);
    const parsed = await listSpaceBoardStates(client, "anchor123");
    expect(parsed[0]?.group).toBe("backlog");
    expect(transport.calls.at(-1)?.url).toBe("https://api.test/api/public/anchor/anchor123/states/");
  });

  it("lists labels with an optional parent", async () => {
    const { client, transport } = clientFor([
      { id: UUID, name: "Bug", color: "#e11d48", parent: null },
      { id: UUID2, name: "Child", color: "#333333", parent: UUID },
    ]);
    const parsed = await listSpaceBoardLabels(client, "anchor123");
    expect(parsed[1]?.parent).toBe(UUID);
    expect(transport.calls.at(-1)?.url).toBe("https://api.test/api/public/anchor/anchor123/labels/");
  });

  it("lists members with the column-name projection", async () => {
    const { client, transport } = clientFor([
      { id: UUID, member: UUID2, member__display_name: "Tester", member__avatar: "" },
    ]);
    const parsed = await listSpaceBoardMembers(client, "anchor123");
    expect(parsed[0]?.member__display_name).toBe("Tester");
    expect(transport.calls.at(-1)?.url).toBe("https://api.test/api/public/anchor/anchor123/members/");
  });

  it("lists cycles and modules with their minimal projection", async () => {
    const cycles = clientFor([{ id: UUID, name: "Sprint 1" }]);
    expect((await listSpaceBoardCycles(cycles.client, "anchor123"))[0]?.name).toBe("Sprint 1");
    expect(cycles.transport.calls.at(-1)?.url).toBe("https://api.test/api/public/anchor/anchor123/cycles/");

    const modules = clientFor([{ id: UUID2, name: "Core" }]);
    expect((await listSpaceBoardModules(modules.client, "anchor123"))[0]?.name).toBe("Core");
    expect(modules.transport.calls.at(-1)?.url).toBe("https://api.test/api/public/anchor/anchor123/modules/");
  });

  it("rejects a state with an unknown group", async () => {
    const { client } = clientFor([{ name: "Weird", group: "nonsense", color: "#000000", id: UUID, sequence: 1 }]);
    await expect(listSpaceBoardStates(client, "anchor123")).rejects.toThrow();
  });
});
