// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CREATION_NOTICE_JOIN_LABEL, InOnboardingCreationNotice } from "./InOnboardingCreationNotice.js";

describe("InOnboardingCreationNotice", () => {
  it("steers to joining through its single action", async () => {
    const user = userEvent.setup();
    const onJoin = vi.fn();
    render(<InOnboardingCreationNotice onJoin={onJoin} />);
    expect(screen.getByRole("heading", { name: "Workspace creation is turned off" })).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: CREATION_NOTICE_JOIN_LABEL }));
    expect(onJoin).toHaveBeenCalledTimes(1);
  });
});
