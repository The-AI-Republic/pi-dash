// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Upkeep tab body (parity/specs/profile.md §11, PROF-024): the quiet
// loading frame, the empty explanation, one card per job in server order,
// the master switch reading as paused, and per-job switches that go
// read-only while pausing.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { UpkeepSettings } from "@pidash/api-client";

const harness = vi.hoisted(() => ({
  settings: { data: undefined as UpkeepSettings | undefined },
  setPaused: { mutate: vi.fn() },
  setJob: { mutate: vi.fn() },
}));

vi.mock("../api.js", () => ({
  useUpkeepSettings: () => harness.settings,
  useSetUpkeepPaused: () => harness.setPaused,
  useSetUpkeepJobEnabled: () => harness.setJob,
}));

import { UPKEEP_EMPTY_EXPLANATION } from "../lib/copy.js";
import { UpkeepSettingsLoading, UpkeepSettingsPage } from "./UpkeepSettingsPage.js";

function settings(overrides: Partial<UpkeepSettings> = {}): UpkeepSettings {
  return {
    enabled: true,
    jobs: [
      { slug: "triage", name: "Triage", description: "Sort new issues.", interval_label: "Daily", enabled: true },
      { slug: "digest", name: "Digest", description: "Summarise the week.", interval_label: "Weekly", enabled: false },
    ],
    ...overrides,
  };
}

beforeEach(() => {
  harness.settings = { data: settings() };
  harness.setPaused.mutate = vi.fn();
  harness.setJob.mutate = vi.fn();
});

describe("UpkeepSettingsPage", () => {
  it("holds a quiet frame before the read", () => {
    harness.settings = { data: undefined };
    render(<UpkeepSettingsPage />);
    expect(screen.getByTestId("upkeep-settings-loading")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("upkeep-settings-page")).not.toBeInTheDocument();
  });

  it("renders the three-section frame on its own", () => {
    render(<UpkeepSettingsLoading />);
    const frame = screen.getByTestId("upkeep-settings-loading");
    expect(frame).toHaveAttribute("aria-busy", "true");
    expect(frame.children.length).toBe(3);
  });

  it("explains an instance with nothing scheduled", () => {
    harness.settings = { data: settings({ jobs: [] }) };
    render(<UpkeepSettingsPage />);
    expect(screen.getByText(UPKEEP_EMPTY_EXPLANATION)).toBeInTheDocument();
    expect(screen.queryByTestId("upkeep-master-switch")).not.toBeInTheDocument();
    expect(screen.queryByTestId("upkeep-job-list")).not.toBeInTheDocument();
  });

  it("lists one card per job in server order with its name, interval and description", () => {
    render(<UpkeepSettingsPage />);
    const jobs = screen.getAllByTestId("upkeep-job");
    expect(jobs).toHaveLength(2);
    expect(jobs[0]).toHaveAttribute("data-job-slug", "triage");
    expect(within(jobs[0]!).getByText("Triage")).toBeInTheDocument();
    expect(within(jobs[0]!).getByText("Daily")).toBeInTheDocument();
    expect(within(jobs[0]!).getByText("Sort new issues.")).toBeInTheDocument();
    expect(jobs[1]).toHaveAttribute("data-job-slug", "digest");
  });

  it("shows each job's own switch state", () => {
    render(<UpkeepSettingsPage />);
    expect(screen.getByTestId("upkeep-job-switch-triage")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("upkeep-job-switch-digest")).toHaveAttribute("aria-checked", "false");
  });

  it("reads the master switch as paused only while the server says so", () => {
    render(<UpkeepSettingsPage />);
    expect(screen.getByTestId("upkeep-master-switch")).toHaveAttribute("aria-checked", "false");
    harness.settings = { data: settings({ enabled: false }) };
    render(<UpkeepSettingsPage />);
    expect(screen.getAllByTestId("upkeep-master-switch")[1]).toHaveAttribute("aria-checked", "true");
  });

  it("flips the master through its hook without touching jobs", async () => {
    const user = userEvent.setup();
    render(<UpkeepSettingsPage />);
    await user.click(screen.getByTestId("upkeep-master-switch"));
    expect(harness.setPaused.mutate).toHaveBeenCalledWith(true);
    expect(harness.setJob.mutate).not.toHaveBeenCalled();
  });

  it("flips one job with its slug and next state", async () => {
    const user = userEvent.setup();
    render(<UpkeepSettingsPage />);
    await user.click(screen.getByTestId("upkeep-job-switch-triage"));
    expect(harness.setJob.mutate).toHaveBeenCalledWith({ slug: "triage", enabled: false });
    await user.click(screen.getByTestId("upkeep-job-switch-digest"));
    expect(harness.setJob.mutate).toHaveBeenLastCalledWith({ slug: "digest", enabled: true });
  });

  it("binds no shortcuts, drops or file inputs (PROF-027)", async () => {
    const user = userEvent.setup();
    const { container } = render(<UpkeepSettingsPage />);
    expect(container.querySelectorAll('input[type="file"]')).toHaveLength(0);
    expect(container.querySelectorAll("[draggable='true']")).toHaveLength(0);
    await user.keyboard("{Control>}p{/Control}");
    expect(harness.setPaused.mutate).not.toHaveBeenCalled();
    expect(harness.setJob.mutate).not.toHaveBeenCalled();
  });

  it("makes job switches read-only while paused, keeping their own states", async () => {
    const user = userEvent.setup();
    harness.settings = { data: settings({ enabled: false }) };
    render(<UpkeepSettingsPage />);
    expect(screen.getByTestId("upkeep-job-switch-triage")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("upkeep-job-switch-triage")).toHaveAttribute("tabindex", "-1");
    expect(screen.getByTestId("upkeep-job-switch-digest")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("upkeep-job-switch-triage")).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByTestId("upkeep-job-switch-triage"));
    expect(harness.setJob.mutate).not.toHaveBeenCalled();
  });
});
