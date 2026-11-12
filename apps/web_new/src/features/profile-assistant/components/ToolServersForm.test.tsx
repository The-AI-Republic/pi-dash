// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server registry (parity/specs/profile.md §10, PROF-023): each row
// carries its name, endpoint, disabled marker, credential lock and the
// tool prefix it claims; adding needs a name and an endpoint.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { McpServer } from "@pidash/api-client";

import {
  AUTH_HEADER_PLACEHOLDER,
  SERVER_DISABLED_MARKER,
  SERVER_LOCK_MARKER,
  SERVER_NAME_PLACEHOLDER,
  SERVER_URL_PLACEHOLDER,
  TOOLS_SECTION_HEADING,
} from "../lib/copy.js";
import {
  ToolServersForm,
  emptyToolServerDraft,
  type ToolServerDraft,
  type ToolServersFormProps,
} from "./ToolServersForm.js";

const ID_ONE = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";
const ID_TWO = "8a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

function server(overrides: Partial<McpServer> = {}): McpServer {
  return {
    id: ID_ONE,
    name: "probe",
    url: "https://mcp.test",
    has_auth_header: false,
    tool_prefix: "probe",
    effective_tool_prefix: "probe",
    is_enabled: true,
    created_at: "2026-10-19T00:00:00Z",
    updated_at: "2026-10-19T00:00:00Z",
    ...overrides,
  };
}

function renderForm(overrides: Partial<ToolServersFormProps> = {}) {
  const props: ToolServersFormProps = {
    servers: [],
    draft: emptyToolServerDraft(),
    onDraftChange: vi.fn(),
    onAdd: vi.fn(),
    onToggle: vi.fn(),
    onRemove: vi.fn(),
    adding: false,
    pendingId: null,
    ...overrides,
  };
  render(<ToolServersForm {...props} />);
  return props;
}

describe("ToolServersForm", () => {
  it("shows the placeholder hints while nothing is registered", () => {
    renderForm();
    expect(screen.getByRole("heading", { name: TOOLS_SECTION_HEADING })).toBeInTheDocument();
    expect(screen.queryByTestId("tool-server-list")).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText(SERVER_NAME_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(SERVER_URL_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(AUTH_HEADER_PLACEHOLDER)).toHaveAttribute("type", "password");
  });

  it("renders each row's name, endpoint and claimed prefix", () => {
    renderForm({ servers: [server()] });
    const row = screen.getByTestId("tool-server-row");
    expect(within(row).getAllByText("probe")[0]).toBeInTheDocument();
    expect(within(row).getByText("https://mcp.test")).toHaveClass("truncate");
    expect(within(row).getByText("probe_*")).toBeInTheDocument();
    expect(within(row).queryByText(SERVER_DISABLED_MARKER)).not.toBeInTheDocument();
  });

  it("marks a disabled row and hides its claimed prefix", () => {
    renderForm({ servers: [server({ is_enabled: false, effective_tool_prefix: null })] });
    const row = screen.getByTestId("tool-server-row");
    expect(within(row).getByText(SERVER_DISABLED_MARKER)).toBeInTheDocument();
    expect(within(row).queryByText("probe_*")).not.toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Enable probe" })).toBeInTheDocument();
  });

  it("marks a row with a stored credential and never shows the value", () => {
    renderForm({ servers: [server({ has_auth_header: true })] });
    const row = screen.getByTestId("tool-server-row");
    expect(within(row).getByText(SERVER_LOCK_MARKER)).toHaveAttribute("aria-label", "Credentials saved");
    expect(within(row).queryByText(/Bearer/)).not.toBeInTheDocument();
  });

  it("falls back to the row's own slug when no effective prefix is resolved", () => {
    renderForm({ servers: [server({ effective_tool_prefix: null, tool_prefix: "slug" })] });
    expect(screen.getByText("slug_*")).toBeInTheDocument();
  });

  it("gates add on a name and an endpoint", () => {
    renderForm({ draft: { name: "probe", url: "", authHeader: "" } });
    expect(screen.getByRole("button", { name: "Add server" })).toBeDisabled();
    renderForm({ draft: { name: "", url: "https://mcp.test", authHeader: "" } });
    expect(screen.getAllByRole("button", { name: "Add server" })[1]).toBeDisabled();
    renderForm({ draft: { name: "probe", url: "https://mcp.test", authHeader: "" } });
    expect(screen.getAllByRole("button", { name: "Add server" })[2]).toBeEnabled();
  });

  it("reports draft edits and fires add, toggle and remove", async () => {
    const user = userEvent.setup();
    const draft: ToolServerDraft = { name: "", url: "", authHeader: "" };
    const props = renderForm({ servers: [server()], draft });
    await user.type(screen.getByLabelText("Name"), "p");
    expect(props.onDraftChange).toHaveBeenLastCalledWith({ name: "p", url: "", authHeader: "" });
    await user.type(screen.getByLabelText("Authorization header (optional)"), "h");
    expect(props.onDraftChange).toHaveBeenLastCalledWith({ name: "", url: "", authHeader: "h" });
    await user.click(screen.getByRole("button", { name: "Disable probe" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(props.onToggle).toHaveBeenCalledWith(server());
    expect(props.onRemove).toHaveBeenCalledWith(server());
    expect(props.onAdd).not.toHaveBeenCalled();

    const ready = renderForm({ draft: { name: "probe", url: "https://mcp.test", authHeader: "" } });
    const addButtons = screen.getAllByRole("button", { name: "Add server" });
    await user.click(addButtons[addButtons.length - 1]!);
    expect(ready.onAdd).toHaveBeenCalledTimes(1);
  });

  it("marks only the pending row busy and blocks its removal", () => {
    renderForm({
      servers: [server(), server({ id: ID_TWO, name: "second" })],
      pendingId: ID_ONE,
    });
    const rows = screen.getAllByTestId("tool-server-row");
    expect(within(rows[0]!).getByRole("button", { name: "Disable probe" })).toHaveAttribute("aria-busy", "true");
    expect(within(rows[0]!).getByRole("button", { name: "Remove" })).toBeDisabled();
    expect(within(rows[1]!).getByRole("button", { name: "Remove" })).toBeEnabled();
  });

  it("marks the add control busy while the request is in flight", () => {
    renderForm({ draft: { name: "probe", url: "https://mcp.test", authHeader: "" }, adding: true });
    expect(screen.getByRole("button", { name: "Add server" })).toBeDisabled();
  });

  it("keeps every row when the list is undefined", () => {
    renderForm({ servers: undefined });
    expect(screen.queryByTestId("tool-server-list")).not.toBeInTheDocument();
  });
});
