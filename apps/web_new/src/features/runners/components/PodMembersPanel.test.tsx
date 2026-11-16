// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Pod, Runner } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { POD_ADD_RUNNER_LABEL, POD_ADD_RUNNER_NONE, POD_MEMBERS_EMPTY, PodMembersPanel } from "./PodMembersPanel.js";

const podA = { id: "pod-a", name: "WEB_a", project: "prj", project_identifier: "WEB" } as Pod;
const podB = { id: "pod-b", name: "WEB_b", project: "prj", project_identifier: "WEB" } as Pod;

function runner(overrides: Partial<Runner> = {}): Runner {
  return {
    id: "r-1",
    name: "codex-1",
    provisioning: "manual",
    pod_dispatch: "pod_member",
    owner: "me",
    pod: "pod-a",
    pod_detail: { id: "pod-a", name: "WEB_a", is_default: true, project: "prj", project_identifier: "WEB" },
    dev_machine_detail: null,
    host_label: "",
    revoked_at: null,
    ...overrides,
  } as Runner;
}

function setup(runners: Runner[], userId: string | null = "me") {
  const onUpdate = vi.fn(async () => undefined);
  render(<PodMembersPanel pod={podA} runners={runners} pods={[podA, podB]} userId={userId} onUpdate={onUpdate} />);
  return onUpdate;
}

describe("PodMembersPanel", () => {
  it("adds a runner from another pod of the project by moving it in", async () => {
    const elsewhere = runner({
      id: "r-engine",
      name: "builtin-x",
      provisioning: "desktop_bundled",
      pod_dispatch: "pinned_only",
      pod: "pod-b",
      pod_detail: { id: "pod-b", name: "WEB_b", is_default: false, project: "prj", project_identifier: "WEB" },
      dev_machine_detail: { id: "m-1", label: "Work laptop", host_label: "rich-mbp" },
    });
    const onUpdate = setup([elsewhere]);
    expect(screen.getByText(POD_MEMBERS_EMPTY)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: POD_ADD_RUNNER_LABEL }));
    await userEvent.click(await screen.findByRole("menuitem", { name: /Built-in engine · Work laptop/ }));
    expect(onUpdate).toHaveBeenCalledWith("r-engine", { pod: "pod-a", pod_dispatch: "pod_member" });
  });

  it("moves a member to another pod of its project", async () => {
    const onUpdate = setup([runner()]);
    await userEvent.click(screen.getByRole("button", { name: "Move to pod: codex-1" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "WEB_b" }));
    expect(onUpdate).toHaveBeenCalledWith("r-1", { pod: "pod-b" });
  });

  it("says when nothing can be added, and hides actions without a signed-in user", async () => {
    setup([runner()]);
    await userEvent.click(screen.getByRole("button", { name: POD_ADD_RUNNER_LABEL }));
    expect(await screen.findByRole("menuitem", { name: POD_ADD_RUNNER_NONE })).toBeInTheDocument();
  });

  it("is read-only for a viewer who owns nothing", () => {
    setup([runner()], null);
    expect(screen.queryByRole("button", { name: POD_ADD_RUNNER_LABEL })).toBeNull();
    expect(screen.queryByRole("button", { name: "Remove from pod" })).toBeNull();
    expect(screen.getByText("Takes pod work")).toBeInTheDocument();
  });
});
