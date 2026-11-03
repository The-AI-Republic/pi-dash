// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Runner } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const useDocumentHead = vi.fn();
vi.mock("../../../shared/shell/index.js", () => ({
  useDocumentHead: (head: unknown) => useDocumentHead(head),
}));

const linkSpy = vi.fn();
vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => {
    linkSpy(to);
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

import {
  DASH,
  RUNNER_DETAIL_BACK,
  RUNNER_DETAIL_CHAT,
  RUNNER_DETAIL_FAILED,
  RUNNER_DETAIL_LOADING,
  RunnerDetailScreen,
  enrolledLabel,
  machineLabel,
  platformLabel,
  runnerDetailDocumentTitle,
} from "./RunnerDetailScreen.js";

function runner(overrides: Partial<Runner> = {}): Runner {
  return {
    id: "runner-1",
    name: "alpha",
    status: "online",
    host_label: "host-a",
    provisioning: "manual",
    os: "linux",
    arch: "arm64",
    runner_version: "9.9.9",
    dev_metadata: { working_dir: "/tmp/wd" },
    protocol_version: 3,
    capabilities: ["cap-a", "cap-b"],
    last_heartbeat_at: "2026-01-01T00:00:00Z",
    owner: "owner-1",
    dev_machine: "machine-1",
    dev_machine_detail: { id: "machine-1", host_label: "host-a", label: "build box" },
    visibility: 0,
    pod: "pod-1",
    pod_detail: { id: "pod-1", name: "WEB_alpha", is_default: false, project: "prj-1", project_identifier: "WEB" },
    live_state: null,
    enrolled_at: "2026-01-01T00:00:00Z",
    revoked_at: null,
    revoked_reason: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    ...overrides,
  };
}

function meta(): Record<string, string> {
  const section = screen.getByText("Metadata").closest("section") as HTMLElement;
  const out: Record<string, string> = {};
  for (const term of section.querySelectorAll("dt")) {
    out[(term.textContent ?? "").trim()] = (term.nextElementSibling?.textContent ?? "").trim().replace(/\s+/g, " ");
  }
  return out;
}

function setup(overrides: Partial<React.ComponentProps<typeof RunnerDetailScreen>> = {}) {
  return render(
    <RunnerDetailScreen
      workspaceSlug="acme"
      runnerId="runner-1"
      runner={runner()}
      loading={false}
      failed={false}
      {...overrides}
    />
  );
}

describe("runner detail helpers", () => {
  it("names the document and falls back before the data arrives", () => {
    expect(runnerDetailDocumentTitle(undefined)).toBe("Runner detail");
    expect(runnerDetailDocumentTitle("alpha")).toBe("Runner alpha");
  });

  it("formats platform, machine and enrolment fallbacks", () => {
    expect(platformLabel({ os: "", arch: "" })).toBe(DASH);
    expect(platformLabel({ os: "linux", arch: "arm64" })).toBe("linux / arm64");
    expect(machineLabel({ dev_machine_detail: null })).toBe(DASH);
    expect(machineLabel({ dev_machine_detail: { id: "m", host_label: "", label: "" } })).toBe(DASH);
    expect(machineLabel({ dev_machine_detail: { id: "m", host_label: "host", label: "" } })).toBe("host");
    expect(enrolledLabel({ enrolled_at: null })).toBe("Pending enrollment");
    expect(enrolledLabel({ enrolled_at: "2026-01-01T00:00:00Z" }, () => "Jan 1")).toBe("Jan 1");
  });
});

describe("RunnerDetailScreen", () => {
  it("shows the header, metadata grid and activity panel", () => {
    setup();
    expect(screen.getByRole("heading", { name: "alpha" })).toBeInTheDocument();
    expect(screen.getByText("online")).toBeInTheDocument();
    const rows = meta();
    expect(rows).toMatchObject({
      "Runner ID": "runner-1",
      Pod: "WEB_alpha",
      Project: "WEB",
      "Dev machine": "build box",
      "OS / Arch": "linux / arm64",
      Version: "9.9.9",
      "Working directory": "/tmp/wd",
      "Protocol version": "3",
      Capabilities: "cap-acap-b",
      Connection: "—",
      Owner: "owner-1",
    });
    expect(rows["Last heartbeat"]).not.toBe("—");
    expect(rows["Enrolled at"]).not.toBe("Pending enrollment");
    expect(rows["Created at"]).not.toBe("—");
    expect(rows["Updated at"]).not.toBe("—");
    expect(screen.getByRole("region", { name: "Agent activity" })).toBeInTheDocument();
  });

  it("renders pending enrollment, dashes and revocation rows", () => {
    setup({
      runner: runner({
        dev_machine_detail: null,
        os: "",
        arch: "",
        runner_version: "",
        dev_metadata: {},
        capabilities: [],
        last_heartbeat_at: null,
        enrolled_at: null,
        revoked_at: "2026-01-01T00:00:00Z",
        revoked_reason: "parity-probe-revoke",
      }),
    });
    const rows = meta();
    expect(rows["Dev machine"]).toBe(DASH);
    expect(rows["OS / Arch"]).toBe(DASH);
    expect(rows.Version).toBe(DASH);
    expect(rows["Working directory"]).toBe(DASH);
    expect(rows.Capabilities).toBe(DASH);
    expect(rows["Last heartbeat"]).toBe(DASH);
    expect(rows["Enrolled at"]).toBe("Pending enrollment");
    expect(rows["Revoked at"]).not.toBe(DASH);
    expect(rows["Revoked reason"]).toBe("parity-probe-revoke");
  });

  it("links back to the workspace list and into chat", () => {
    setup();
    expect(screen.getByRole("link", { name: RUNNER_DETAIL_BACK })).toHaveAttribute("href", "/acme/runners");
    expect(screen.getByRole("link", { name: RUNNER_DETAIL_CHAT })).toHaveAttribute(
      "href",
      "/acme/runners/chat/runner-1"
    );
  });

  it("links back to the project list when project-scoped", () => {
    setup({ projectId: "prj-1" });
    expect(screen.getByRole("link", { name: RUNNER_DETAIL_BACK })).toHaveAttribute(
      "href",
      "/acme/projects/prj-1/runners"
    );
    expect(screen.getByRole("link", { name: RUNNER_DETAIL_CHAT })).toHaveAttribute(
      "href",
      "/acme/projects/prj-1/runners/chat/runner-1"
    );
  });

  it("renders the loading and failure states", () => {
    const { rerender } = setup({ runner: undefined, loading: true });
    expect(screen.getByText(RUNNER_DETAIL_LOADING)).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: RUNNER_DETAIL_CHAT })).not.toBeInTheDocument();

    rerender(<RunnerDetailScreen workspaceSlug="acme" runnerId="runner-1" runner={undefined} loading={false} failed />);
    expect(screen.getByText(RUNNER_DETAIL_FAILED)).toBeInTheDocument();
  });

  it("renders an empty body when nothing has loaded but nothing failed", () => {
    setup({ runner: undefined, loading: false, failed: false });
    expect(screen.queryByText(RUNNER_DETAIL_LOADING)).not.toBeInTheDocument();
    expect(screen.queryByText(RUNNER_DETAIL_FAILED)).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Metadata" })).not.toBeInTheDocument();
  });
});
