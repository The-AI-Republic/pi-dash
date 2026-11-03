// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section mount (NEWFRONT-540, RUN-003). Child routes that later slices own
// render their heading here so the tab addresses resolve and deep-link.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { RunnersAreaSectionMount } from "./RunnersAreaSectionMount.js";

describe("RunnersAreaSectionMount", () => {
  it("renders the section heading", () => {
    render(<RunnersAreaSectionMount title="Runs" />);
    expect(screen.getByRole("heading", { name: "Runs" })).toBeInTheDocument();
  });
});
