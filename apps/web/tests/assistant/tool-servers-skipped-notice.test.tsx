/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ToolServersSkippedNotice } from "@/components/assistant/tool-servers-skipped-notice";

describe("ToolServersSkippedNotice", () => {
  it("maps a known reason code to readable text", () => {
    render(<ToolServersSkippedNotice servers={[{ name: "Jira Tools", reason: "url_blocked" }]} />);
    expect(
      screen.getByText("Tool server Jira Tools was unavailable for this reply (its URL is not allowed).")
    ).toBeTruthy();
  });

  it("maps the surfaced crypto AssistantError code", () => {
    render(<ToolServersSkippedNotice servers={[{ name: "Secure Tools", reason: "assistant_not_configured" }]} />);
    expect(
      screen.getByText(
        "Tool server Secure Tools was unavailable for this reply (its credential could not be decrypted)."
      )
    ).toBeTruthy();
  });

  it("phrases the resolver total-failure sentinel as a whole-capability outage", () => {
    render(<ToolServersSkippedNotice servers={[{ name: "all tool servers", reason: "toolsets_unavailable" }]} />);
    expect(screen.getByText("Tool servers were unavailable for this reply (tools were unavailable).")).toBeTruthy();
  });

  it("renders the cloud openhub reason", () => {
    render(<ToolServersSkippedNotice servers={[{ name: "OpenHub apps", reason: "openhub_unavailable" }]} />);
    expect(
      screen.getByText("Tool server OpenHub apps was unavailable for this reply (OpenHub apps were unavailable).")
    ).toBeTruthy();
  });

  it("falls back to the raw reason for an unknown code (e.g. a runtime exception name)", () => {
    render(<ToolServersSkippedNotice servers={[{ name: "Flaky Tools", reason: "ConnectionError" }]} />);
    expect(screen.getByText("Tool server Flaky Tools was unavailable for this reply (ConnectionError).")).toBeTruthy();
  });

  it("renders one line per skipped server", () => {
    render(
      <ToolServersSkippedNotice
        servers={[
          { name: "Jira Tools", reason: "url_blocked" },
          { name: "Confluence", reason: "toolset_unavailable" },
        ]}
      />
    );
    expect(
      screen.getByText("Tool server Jira Tools was unavailable for this reply (its URL is not allowed).")
    ).toBeTruthy();
    expect(
      screen.getByText("Tool server Confluence was unavailable for this reply (it could not be reached).")
    ).toBeTruthy();
  });

  it("renders nothing when there are no servers", () => {
    const { container } = render(<ToolServersSkippedNotice servers={[]} />);
    expect(container.firstChild).toBeNull();
  });
});
