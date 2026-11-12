// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item-type surfaces on the detail (ISS-235): with no flag or no
// provider the identifier stays bare and the property and activity slots
// are empty; a cloud build mounts whichever pieces it registered.
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { edition } from "../../../core/edition/index.js";
import { DetailTypeActivity, DetailTypeBadge, DetailTypeProperties, hasDetailTypeSurfaces } from "./DetailTypeSlots.js";
import { ISSUE_TYPES_FLAG } from "./flags.js";
import type { IssueTypeDetailSlot } from "./providers.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";

afterEach(() => {
  cleanup();
  delete edition.flags[ISSUE_TYPES_FLAG];
  resetEditionIssuesProviders();
});

const BASE = { workspaceSlug: "acme", projectId: "p-1", issueId: "i-1", editable: true };

function slot(): IssueTypeDetailSlot {
  return {
    badge: ({ editable }) => <span>{editable ? "Bug (switcher)" : "Bug"}</span>,
    properties: () => <span>Severity</span>,
    activity: ({ issueId }) => <span>{`type changed ${issueId}`}</span>,
  };
}

function renderAll(): void {
  render(
    <>
      <DetailTypeBadge {...BASE} />
      <DetailTypeProperties {...BASE} />
      <DetailTypeActivity {...BASE} />
    </>
  );
}

describe("DetailTypeSlots", () => {
  it("renders nothing in the community build", () => {
    registerEditionIssuesProviders({ issueTypeDetail: slot() });
    renderAll();
    expect(screen.queryByTestId("detail-type-badge")).toBeNull();
    expect(screen.queryByTestId("detail-type-properties")).toBeNull();
    expect(screen.queryByTestId("detail-type-activity")).toBeNull();
    expect(hasDetailTypeSurfaces()).toBe(false);
  });

  it("renders nothing with the flag on but no provider", () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    renderAll();
    expect(hasDetailTypeSurfaces()).toBe(false);
    expect(screen.queryByTestId("detail-type-badge")).toBeNull();
  });

  it("renders nothing when the provider registers no pieces", () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    registerEditionIssuesProviders({ issueTypeDetail: {} });
    renderAll();
    expect(hasDetailTypeSurfaces()).toBe(false);
  });

  it("mounts each registered piece with the row and its edit standing", () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    registerEditionIssuesProviders({ issueTypeDetail: slot() });
    renderAll();
    expect(hasDetailTypeSurfaces()).toBe(true);
    expect(screen.getByTestId("detail-type-badge")).toHaveTextContent("Bug (switcher)");
    expect(screen.getByTestId("detail-type-properties")).toHaveTextContent("Severity");
    expect(screen.getByTestId("detail-type-activity")).toHaveTextContent("type changed i-1");
  });

  it("mounts a single registered piece on its own", () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    registerEditionIssuesProviders({ issueTypeDetail: { badge: () => <span>Bug</span> } });
    renderAll();
    expect(screen.getByTestId("detail-type-badge")).toHaveTextContent("Bug");
    expect(screen.queryByTestId("detail-type-properties")).toBeNull();
    expect(screen.queryByTestId("detail-type-activity")).toBeNull();
  });

  it("treats the other pieces as absent when one is missing from a busy slot", () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    const { badge: _badge, activity: _activity, ...onlyProperties } = slot();
    registerEditionIssuesProviders({ issueTypeDetail: onlyProperties });
    renderAll();
    expect(screen.queryByTestId("detail-type-badge")).toBeNull();
    expect(screen.getByTestId("detail-type-properties")).toHaveTextContent("Severity");
    expect(screen.queryByTestId("detail-type-activity")).toBeNull();
  });

  it("passes read-only standing through to the badge", () => {
    edition.flags[ISSUE_TYPES_FLAG] = true;
    registerEditionIssuesProviders({ issueTypeDetail: slot() });
    render(<DetailTypeBadge {...BASE} editable={false} />);
    expect(screen.getByTestId("detail-type-badge")).toHaveTextContent("Bug");
  });
});
