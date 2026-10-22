// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));
vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

import { ViewAccessSummary } from "./ViewAccessSummary.js";

afterEach(() => {
  harness.edition = { id: "oss", flags: {} };
});

describe("ViewAccessSummary", () => {
  it("renders nothing on OSS", () => {
    render(<ViewAccessSummary access={1} />);
    expect(screen.queryByTestId("view-access-summary")).toBeNull();
  });

  it("shows the stored access read-only on cloud", () => {
    harness.edition = { id: "cloud", flags: {}, slots: { viewAccess: {} } };
    render(<ViewAccessSummary access={0} />);
    expect(screen.getByTestId("view-access-summary")).toHaveTextContent("Shared with: Private");
  });
});
