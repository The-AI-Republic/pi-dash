// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { DetailMissingState } from "./DetailMissingState.js";
import { DetailSkeleton } from "./DetailSkeleton.js";

describe("DetailSkeleton", () => {
  it("announces the loading detail", () => {
    render(<DetailSkeleton />);
    expect(screen.getByRole("status", { name: "Loading work item" })).toBeInTheDocument();
  });
});

describe("DetailMissingState", () => {
  it("names the key and renders the way out", () => {
    render(
      <DetailMissingState issueKey="WEB-9">
        <a href="/acme">Back to the workspace home</a>
      </DetailMissingState>
    );
    expect(screen.getByRole("heading", { name: "No work item matches this address" })).toBeInTheDocument();
    expect(screen.getByText(/Nothing is filed under WEB-9/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to the workspace home" })).toBeInTheDocument();
  });

  it("explains short-links with the archived-or-deleted wording", () => {
    render(
      <DetailMissingState issueKey="some-id" body="It may have been archived or deleted, or the address is wrong.">
        <a href="/acme">Back</a>
      </DetailMissingState>
    );
    expect(screen.getByText("It may have been archived or deleted, or the address is wrong.")).toBeInTheDocument();
  });
});
