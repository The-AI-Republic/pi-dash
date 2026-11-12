// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared unit-test harness for the runner-chat hooks and page: a fresh
// QueryClient per render, a stubbed app client, and minimally complete
// runner/session factories. Test-only; never imported by app code.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { RenderHookOptions } from "@testing-library/react";
import * as React from "react";

import type { ChatSession, Runner } from "@pidash/api-client";

export function testClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

type WrapperComponent = NonNullable<RenderHookOptions<unknown>["wrapper"]>;

// `@tanstack/react-query` types its children against the React 18 types pnpm
// resolves for it, so the provider is re-typed against this app's React 19.
const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: QueryClient;
  children?: React.ReactNode;
}>;

export function wrapper(client: QueryClient): WrapperComponent {
  return function Wrapper({ children }) {
    return <Provider client={client}>{children}</Provider>;
  };
}

export function makeRunner(overrides: Partial<Runner> = {}): Runner {
  return {
    id: "r-1",
    name: "alpha",
    status: "online",
    host_label: "host",
    provisioning: "manual",
    os: "linux",
    arch: "x86_64",
    runner_version: "1.0.0",
    dev_metadata: {},
    protocol_version: 1,
    capabilities: [],
    last_heartbeat_at: null,
    owner: "u-1",
    dev_machine: null,
    dev_machine_detail: null,
    visibility: 0,
    pod: "p-1",
    pod_detail: {
      id: "p-1",
      name: "Default",
      is_default: true,
      project: "proj",
      project_identifier: "PROJ",
    },
    live_state: null,
    enrolled_at: "2026-01-01T00:00:00Z",
    revoked_at: null,
    revoked_reason: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

export function makeSession(overrides: Partial<ChatSession> = {}): ChatSession {
  return {
    id: "s-1",
    workspace: "ws-id",
    runner: "r-1",
    runner_detail: { id: "r-1", name: "alpha", status: "online" },
    created_by: "u-1",
    pod: "p-1",
    status: "open",
    agent_kind: "codex",
    local_thread_id: "",
    local_session_id: "",
    cwd: "",
    model: "",
    active_turn_id: "",
    active_message_id: null,
    close_requested: false,
    last_message_at: "2026-10-18T11:00:00Z",
    closed_at: null,
    error: "",
    created_at: "2026-10-18T10:00:00Z",
    updated_at: "2026-10-18T11:00:00Z",
    ...overrides,
  };
}
