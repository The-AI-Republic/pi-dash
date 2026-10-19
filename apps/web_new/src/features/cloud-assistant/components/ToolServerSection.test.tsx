// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server section (CLOUD-036): the heading cloud users must keep, the
// add gate, row markers, and the outcome of each write.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { notifySuccess, notifyInfo, notifyError } = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/shell/toaster.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/toaster.js")>();
  return { ...original, notifySuccess, notifyInfo, notifyError };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ToolServerSection } from "./ToolServerSection.js";

const SERVERS = [
  {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "probe-mcp-a",
    url: "https://mcp-a.example.com/sse",
    has_auth_header: false,
    tool_prefix: "probe",
    effective_tool_prefix: "probe_*",
    is_enabled: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
  {
    id: "123e4567-e89b-12d3-a456-426614174001",
    name: "probe-mcp-b",
    url: "https://mcp-b.example.com/mcp",
    has_auth_header: true,
    tool_prefix: "probe_b",
    effective_tool_prefix: null,
    is_enabled: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
];

function stubTransport(
  handler: (method: string, url: string, body: unknown) => { status: number; body?: unknown } | undefined
) {
  const calls: Array<{ method: string; url: string; body: unknown }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const call = {
      method: init.method ?? "GET",
      url,
      body: typeof init.body === "string" && init.body !== "" ? (JSON.parse(init.body) as unknown) : null,
    };
    calls.push(call);
    const reply = handler(call.method, call.url, call.body);
    if (reply === undefined) throw new Error(`unexpected call ${call.method} ${url}`);
    const text = reply.body === undefined ? "" : JSON.stringify(reply.body);
    return {
      status: reply.status,
      url,
      headers: { get: () => (reply.body === undefined ? null : "application/json") },
      text: async () => text,
    };
  }) as never);
  return calls;
}

function renderSection() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToolServerSection />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("ToolServerSection", () => {
  it("heads the section the cloud page must keep", async () => {
    stubTransport((method, url) =>
      method === "GET" && url === "/api/users/me/ai-assistant/mcp-servers/" ? { status: 200, body: SERVERS } : undefined
    );
    renderSection();
    expect(await screen.findByRole("heading", { name: "Tool servers (MCP)" })).toBeInTheDocument();
  });

  it("lists rows with their endpoint, prefix, lock and disabled markers", async () => {
    stubTransport((method) => (method === "GET" ? { status: 200, body: SERVERS } : undefined));
    renderSection();
    expect(await screen.findByText("probe-mcp-a")).toBeInTheDocument();
    expect(screen.getByText("https://mcp-a.example.com/sse")).toBeInTheDocument();
    expect(screen.getByText("probe_*")).toBeInTheDocument();
    expect(screen.getByText("probe-mcp-b").parentElement).toHaveTextContent("probe-mcp-b (disabled)");
    expect(screen.getByText("Locked")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Disable" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Enable" })).toBeInTheDocument();
  });

  it("explains an empty registry and gates the add action on name and address", async () => {
    const user = userEvent.setup();
    stubTransport((method) => (method === "GET" ? { status: 200, body: [] } : undefined));
    renderSection();
    expect(await screen.findByText("No tool servers registered yet.")).toBeInTheDocument();
    const add = screen.getByRole("button", { name: "Add server" });
    expect(add).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Name" }), "probe");
    expect(add).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Server address" }), "https://probe.test/mcp");
    expect(add).toBeEnabled();
  });

  it("posts trimmed values with the optional header, then confirms and clears the form", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((method, url) => {
      if (method === "GET") return { status: 200, body: [] };
      if (method === "POST" && url.endsWith("/api/users/me/ai-assistant/mcp-servers/")) {
        return { status: 201, body: SERVERS[1] };
      }
      return undefined;
    });
    renderSection();
    await screen.findByText("No tool servers registered yet.");
    await user.type(screen.getByRole("textbox", { name: "Name" }), "  probe-mcp-b ");
    await user.type(screen.getByRole("textbox", { name: "Server address" }), " https://mcp-b.example.com/mcp ");
    await user.type(screen.getByLabelText("Authorization header (optional)"), " Bearer x ");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Added", "Tool server added."));
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      name: "probe-mcp-b",
      url: "https://mcp-b.example.com/mcp",
      auth_header: "Bearer x",
    });
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Server address" })).toHaveValue("");
  });

  it("omits the header when the field is blank", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((method) => {
      if (method === "GET") return { status: 200, body: [] };
      if (method === "POST") return { status: 201, body: SERVERS[0] };
      return undefined;
    });
    renderSection();
    await screen.findByText("No tool servers registered yet.");
    await user.type(screen.getByRole("textbox", { name: "Name" }), "probe-mcp-a");
    await user.type(screen.getByRole("textbox", { name: "Server address" }), "https://mcp-a.example.com/sse");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      name: "probe-mcp-a",
      url: "https://mcp-a.example.com/sse",
    });
  });

  it("reports a rejected add with the server's own message", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET") return { status: 200, body: [] };
      if (method === "POST" && url.endsWith("/api/users/me/ai-assistant/mcp-servers/")) {
        return { status: 400, body: { detail: "You already have a tool server with that name." } };
      }
      return undefined;
    });
    renderSection();
    await screen.findByText("No tool servers registered yet.");
    await user.type(screen.getByRole("textbox", { name: "Name" }), "probe-mcp-a");
    await user.type(screen.getByRole("textbox", { name: "Server address" }), "https://mcp-a.example.com/other");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Could not add server", "You already have a tool server with that name.")
    );
  });

  it("toggles a row and reports a failed update", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((method, url) => {
      if (method === "GET") return { status: 200, body: SERVERS };
      if (method === "PATCH" && url.includes(SERVERS[0]!.id)) return { status: 400, body: { detail: "Blocked host" } };
      return undefined;
    });
    renderSection();
    await screen.findByText("probe-mcp-a");
    await user.click(screen.getByRole("button", { name: "Disable" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Update failed", "Blocked host"));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ is_enabled: false });
  });

  it("removes a row and reports the outcome", async () => {
    const user = userEvent.setup();
    let servers = SERVERS;
    stubTransport((method, url) => {
      if (method === "GET") return { status: 200, body: servers };
      if (method === "DELETE" && url.includes(SERVERS[1]!.id)) {
        servers = [SERVERS[0]!];
        return { status: 204 };
      }
      return undefined;
    });
    renderSection();
    await screen.findByText("probe-mcp-b");
    await user.click(screen.getAllByRole("button", { name: "Remove" })[1]!);
    await waitFor(() => expect(notifyInfo).toHaveBeenCalledWith("Removed", "probe-mcp-b removed."));
    await waitFor(() => expect(screen.queryByText("probe-mcp-b")).toBeNull());
  });

  it("reports a failed removal", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET") return { status: 200, body: SERVERS };
      if (method === "DELETE" && url.includes(SERVERS[0]!.id)) return { status: 500, body: { detail: "Server error" } };
      return undefined;
    });
    renderSection();
    await screen.findByText("probe-mcp-a");
    await user.click(screen.getAllByRole("button", { name: "Remove" })[0]!);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Remove failed", "Server error"));
  });
});
