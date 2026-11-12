// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The commentary-copy extractor used by the runners driver must read what the
// kit's Dialog actually renders (NEWFRONT-540, RUN-010). These tests build the
// real RunnerConfirmDialog and run the extractor over its DOM.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  RunnerConfirmDialog,
  DELETE_RUNNER_BODY,
  REVOKE_RUNNER_BODY,
} from "../../../src/features/runners/components/RunnerConfirmDialog.js";
import { descriptionAfterHeading } from "./modal-copy.js";

function renderDialog(action: "delete" | "revoke"): HTMLElement {
  render(
    <RunnerConfirmDialog
      open
      action={action}
      runnerName="alpha-runner"
      pending={false}
      onCancel={vi.fn()}
      onConfirm={vi.fn()}
    />
  );
  return screen.getByRole("dialog");
}

describe("descriptionAfterHeading", () => {
  it("reads the delete confirmation copy below the heading", () => {
    const dialog = renderDialog("delete");
    const heading = Array.from(dialog.querySelectorAll("h1, h2, h3, h4")).find(
      (node) => node.textContent === "Delete runner?"
    )!;
    expect(descriptionAfterHeading(heading)).toBe(DELETE_RUNNER_BODY);
  });

  it("reads the revoke confirmation copy below the heading", () => {
    const dialog = renderDialog("revoke");
    const heading = Array.from(dialog.querySelectorAll("h1, h2, h3, h4")).find(
      (node) => node.textContent === "Revoke runner?"
    )!;
    expect(descriptionAfterHeading(heading)).toBe(REVOKE_RUNNER_BODY);
  });

  it("returns an empty string when the heading has no sibling", () => {
    const heading = document.createElement("h2");
    document.body.append(heading);
    expect(descriptionAfterHeading(heading)).toBe("");
  });
});
