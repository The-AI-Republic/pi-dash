// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer-hop note (ISS-236): the cycle row's accent renders only with the
// flag on, a cycle set and a provider registered to say where it came from.
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { edition } from "../../../core/edition/index.js";
import { DETAIL_BADGES_FLAG } from "./flags.js";
import type { DetailBadgeInput } from "./providers.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";
import { TransferHopNote } from "./TransferHopNote.js";

afterEach(() => {
  cleanup();
  delete edition.flags[DETAIL_BADGES_FLAG];
  resetEditionIssuesProviders();
});

function Provider({ issueId, cycleId }: DetailBadgeInput): React.ReactElement {
  return <span>{`${issueId} moved into ${cycleId}`}</span>;
}

const BASE = { workspaceSlug: "acme", projectId: "p-1", issueId: "i-1", cycleId: "c-1" };

describe("TransferHopNote", () => {
  it("renders nothing in the community build", () => {
    registerEditionIssuesProviders({ detailBadges: { transferNote: Provider } });
    render(<TransferHopNote {...BASE} />);
    expect(screen.queryByTestId("detail-transfer-hop")).toBeNull();
  });

  it("renders nothing with the flag on but no provider", () => {
    edition.flags[DETAIL_BADGES_FLAG] = true;
    render(<TransferHopNote {...BASE} />);
    expect(screen.queryByTestId("detail-transfer-hop")).toBeNull();
  });

  it("renders nothing for a row outside any cycle", () => {
    edition.flags[DETAIL_BADGES_FLAG] = true;
    registerEditionIssuesProviders({ detailBadges: { transferNote: Provider } });
    render(<TransferHopNote {...BASE} cycleId={null} />);
    expect(screen.queryByTestId("detail-transfer-hop")).toBeNull();
  });

  it("renders the edition's note for a cycle member", () => {
    edition.flags[DETAIL_BADGES_FLAG] = true;
    registerEditionIssuesProviders({ detailBadges: { transferNote: Provider } });
    render(<TransferHopNote {...BASE} />);
    expect(screen.getByTestId("detail-transfer-hop")).toHaveTextContent("i-1 moved into c-1");
  });
});
