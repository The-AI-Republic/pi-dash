// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The self-contained tool-server registry (parity/specs/profile.md §10,
// PROF-023): it reads its own list, posts the trimmed draft, flips a row
// and removes one by id and name, each while marking that row pending.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { McpServer } from "@pidash/api-client";

const harness = vi.hoisted(() => ({
  servers: { data: undefined as McpServer[] | undefined },
  add: { mutate: vi.fn(), isPending: false },
  toggle: { mutate: vi.fn() },
  remove: { mutate: vi.fn() },
}));

vi.mock("../api/queries.js", () => ({
  useAssistantServers: () => harness.servers,
}));

vi.mock("../api/mutations.js", () => ({
  useAddToolServer: () => harness.add,
  useToggleToolServer: () => harness.toggle,
  useRemoveToolServer: () => harness.remove,
}));

import { SERVER_NAME_PLACEHOLDER, SERVER_URL_PLACEHOLDER, TOOLS_SECTION_HEADING } from "../lib/copy.js";
import { ToolServersSection } from "./ToolServersSection.js";

const SERVER: McpServer = {
  id: "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b",
  name: "probe",
  url: "https://mcp.test",
  has_auth_header: true,
  tool_prefix: "probe",
  effective_tool_prefix: "probe",
  is_enabled: true,
  created_at: "2026-10-19T00:00:00Z",
  updated_at: "2026-10-19T00:00:00Z",
};

beforeEach(() => {
  harness.servers = { data: [SERVER] };
  harness.add = { mutate: vi.fn(), isPending: false };
  harness.toggle = { mutate: vi.fn() };
  harness.remove = { mutate: vi.fn() };
});

describe("ToolServersSection", () => {
  it("renders nothing but the forms while the read has not answered", () => {
    harness.servers = { data: undefined };
    render(<ToolServersSection />);
    expect(screen.getByRole("heading", { name: TOOLS_SECTION_HEADING })).toBeInTheDocument();
    expect(screen.queryByTestId("tool-server-list")).not.toBeInTheDocument();
  });

  it("adds a server with its trimmed fields and clears the draft", async () => {
    const user = userEvent.setup();
    render(<ToolServersSection />);
    await user.type(screen.getByPlaceholderText(SERVER_NAME_PLACEHOLDER), "  team  ");
    await user.type(screen.getByPlaceholderText(SERVER_URL_PLACEHOLDER), "  https://team.test/mcp ");
    await user.type(screen.getByLabelText("Authorization header (optional)"), "  Bearer t  ");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    const [body, options] = harness.add.mutate.mock.calls[0]!;
    expect(body).toEqual({ name: "team", url: "https://team.test/mcp", auth_header: "Bearer t" });
    options.onSuccess();
    await waitFor(() => expect(screen.getByPlaceholderText(SERVER_NAME_PLACEHOLDER)).toHaveValue(""));
    expect(screen.getByPlaceholderText(SERVER_URL_PLACEHOLDER)).toHaveValue("");
  });

  it("omits an empty credential from the add payload", async () => {
    const user = userEvent.setup();
    render(<ToolServersSection />);
    await user.type(screen.getByPlaceholderText(SERVER_NAME_PLACEHOLDER), "team");
    await user.type(screen.getByPlaceholderText(SERVER_URL_PLACEHOLDER), "https://team.test/mcp");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    expect(harness.add.mutate.mock.calls[0]![0]).toEqual({ name: "team", url: "https://team.test/mcp" });
  });

  it("flips the opposite of a row's state and clears its pending mark on settle", async () => {
    const user = userEvent.setup();
    render(<ToolServersSection />);
    await user.click(screen.getByRole("button", { name: "Disable probe" }));
    const [vars, options] = harness.toggle.mutate.mock.calls[0]!;
    expect(vars).toEqual({ serverId: SERVER.id, isEnabled: false });
    expect(screen.getByRole("button", { name: "Disable probe" })).toHaveAttribute("aria-busy", "true");
    options.onSettled();
    await waitFor(() => expect(screen.getByRole("button", { name: "Disable probe" })).not.toHaveAttribute("aria-busy"));
  });

  it("removes a row by id and name, clearing its pending mark", async () => {
    const user = userEvent.setup();
    render(<ToolServersSection />);
    await user.click(screen.getByRole("button", { name: "Remove" }));
    const [vars, options] = harness.remove.mutate.mock.calls[0]!;
    expect(vars).toEqual({ serverId: SERVER.id, name: "probe" });
    options.onSettled();
    await waitFor(() => expect(screen.getByRole("button", { name: "Remove" })).toBeEnabled());
  });
});
