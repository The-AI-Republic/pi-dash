// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity block (PSET-005/PSET-007): the creation stamp, the visibility
// caption, the live name and short code, and who may open the two emblem
// controls. No cover control exists here.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { createdLine, GeneralIdentity, visibilityLabel } from "./GeneralIdentity.js";

const FORMATTER = new Intl.DateTimeFormat("en-US", { dateStyle: "medium", timeZone: "UTC" });

describe("createdLine", () => {
  it("stamps a readable timestamp", () => {
    expect(createdLine("2026-01-02T03:04:05Z", FORMATTER)).toBe("Created on Jan 2, 2026");
  });

  it("reads an unreadable timestamp as empty", () => {
    expect(createdLine("not-a-date", FORMATTER)).toBe("");
  });
});

describe("visibilityLabel", () => {
  it("names public and falls back to private", () => {
    expect(visibilityLabel(2)).toBe("Public");
    expect(visibilityLabel(0)).toBe("Private");
    expect(visibilityLabel(7)).toBe("Private");
  });
});

describe("GeneralIdentity", () => {
  function setup(canEdit: boolean) {
    const onOpenMark = vi.fn();
    const onOpenUpload = vi.fn();
    render(
      <GeneralIdentity
        name="Website relaunch"
        identifier="WEB"
        logoProps={{}}
        network={0}
        createdAt="2026-01-02T03:04:05Z"
        canEdit={canEdit}
        apiBaseUrl="https://api.test"
        onOpenMark={onOpenMark}
        onOpenUpload={onOpenUpload}
      />
    );
    return { onOpenMark, onOpenUpload };
  }

  it("draws the identity, visibility and stamp for an admin with both emblem controls", () => {
    setup(true);
    expect(screen.getByTestId("general-headline")).toHaveTextContent("Website relaunch");
    expect(screen.getByTestId("general-headline-identifier")).toHaveTextContent("WEB");
    expect(screen.getByTestId("general-network")).toHaveTextContent("Private");
    expect(screen.getByTestId("general-created")).toHaveTextContent("Created on");
    expect(screen.getByTestId("general-emblem-open")).toHaveTextContent("Change emblem");
    expect(screen.getByTestId("general-emblem-upload-open")).toHaveTextContent("Upload image");
    expect(screen.queryByText(/cover/i)).not.toBeInTheDocument();
  });

  it("leaves the emblem controls out for a non-admin", () => {
    setup(false);
    expect(screen.queryByTestId("general-emblem-open")).not.toBeInTheDocument();
    expect(screen.queryByTestId("general-emblem-upload-open")).not.toBeInTheDocument();
  });

  it("leaves the creation stamp off when the stored timestamp is unreadable", () => {
    render(
      <GeneralIdentity
        name=""
        identifier=""
        logoProps={{}}
        network={0}
        createdAt="not-a-date"
        canEdit={false}
        apiBaseUrl="https://api.test"
        onOpenMark={vi.fn()}
        onOpenUpload={vi.fn()}
      />
    );
    expect(screen.getByTestId("general-headline")).toHaveTextContent("Untitled project");
    expect(screen.getByTestId("general-headline-identifier")).toHaveTextContent("No short code");
    expect(screen.queryByTestId("general-created")).not.toBeInTheDocument();
  });
});
