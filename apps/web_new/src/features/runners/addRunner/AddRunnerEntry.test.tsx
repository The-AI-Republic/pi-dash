// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview entry (RUN-006): one control opens the modal, locked to the
// enclosing project scope.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const modalProps = vi.fn();
const area = { scope: { workspaceSlug: "acme", projectId: "p-1" as string | undefined }, workspaceId: "ws-id" };
vi.mock("../area.js", () => ({ useRunnersArea: () => area }));
vi.mock("./AddRunnerModal.js", () => ({
  AddRunnerModal: (props: { open: boolean; projectId?: string }) => {
    modalProps(props);
    return <div data-testid="add-runner-modal" data-open={String(props.open)} />;
  },
}));

import { ADD_RUNNER_LABEL, AddRunnerEntry } from "./AddRunnerEntry.js";

describe("AddRunnerEntry", () => {
  it("opens the modal with the enclosing project id", async () => {
    render(<AddRunnerEntry />);
    const control = screen.getByRole("button", { name: ADD_RUNNER_LABEL });
    expect(screen.getByTestId("add-runner-modal")).toHaveAttribute("data-open", "false");
    await userEvent.click(control);
    expect(modalProps).toHaveBeenLastCalledWith(expect.objectContaining({ open: true, projectId: "p-1" }));
  });

  it("passes no project id on the workspace scope", () => {
    area.scope.projectId = undefined;
    render(<AddRunnerEntry />);
    expect(modalProps).toHaveBeenLastCalledWith(expect.objectContaining({ projectId: undefined }));
    area.scope.projectId = "p-1";
  });
});
