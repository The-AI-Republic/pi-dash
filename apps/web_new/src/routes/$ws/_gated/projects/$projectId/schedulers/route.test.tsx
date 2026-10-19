// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers section shell (parity/specs/agents.md §5, AGT-021):
// List and Calendar tabs, exactly one marked current for a tab address and
// neither for an install detail underneath.
import { describe, expect, it } from "vitest";

import { schedulerSectionTabs } from "./route.js";

const BASE = "/acme/projects/p1/schedulers";

describe("schedulerSectionTabs", () => {
  it("marks List current on the list address", () => {
    expect(schedulerSectionTabs(`${BASE}/list`, BASE)).toEqual([
      { key: "list", label: "List", href: `${BASE}/list`, active: true },
      { key: "calendar", label: "Calendar", href: `${BASE}/calendar`, active: false },
    ]);
  });

  it("marks Calendar current on the calendar address", () => {
    const tabs = schedulerSectionTabs(`${BASE}/calendar`, BASE);
    expect(tabs.map((tab) => tab.active)).toEqual([false, true]);
  });

  it("reads the bare section address as Calendar", () => {
    expect(schedulerSectionTabs(BASE, BASE).find((tab) => tab.active)?.key).toBe("calendar");
    expect(schedulerSectionTabs(`${BASE}/`, BASE).find((tab) => tab.active)?.key).toBe("calendar");
  });

  it("tolerates a trailing slash on the list address", () => {
    expect(schedulerSectionTabs(`${BASE}/list/`, BASE).find((tab) => tab.active)?.key).toBe("list");
  });

  it("marks neither tab on an install detail page", () => {
    expect(schedulerSectionTabs(`${BASE}/b1`, BASE).some((tab) => tab.active)).toBe(false);
  });
});
