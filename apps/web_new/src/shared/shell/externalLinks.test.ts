// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { DOCS_URL, FORUM_URL, REPORT_BUG_URL } from "./externalLinks.js";

describe("external help destinations", () => {
  it("points docs, forum and bug reports at the project addresses", () => {
    expect(DOCS_URL).toBe("https://github.com/The-AI-Republic/pi-dash#readme");
    expect(FORUM_URL).toBe("https://github.com/The-AI-Republic/pi-dash/discussions");
    expect(REPORT_BUG_URL).toBe("https://github.com/The-AI-Republic/pi-dash/issues/new/choose");
  });
});
