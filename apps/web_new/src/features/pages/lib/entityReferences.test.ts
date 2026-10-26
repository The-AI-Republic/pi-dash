// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pasted entity-address parsing (parity/specs/pages.md §6.3, PAGE-043).

import { describe, expect, it } from "vitest";

import { parseEntityAddress } from "./entityReferences.js";

describe("parseEntityAddress", () => {
  it("reads a bare work-item key", () => {
    expect(parseEntityAddress("web-42")).toEqual({ key: "WEB-42", projectIdentifier: "WEB", sequenceId: 42 });
  });

  it("reads an app browse address and normalises its key", () => {
    expect(parseEntityAddress("https://app.test/acme/browse/nf-7/")).toEqual({
      key: "NF-7",
      projectIdentifier: "NF",
      sequenceId: 7,
    });
  });

  it("refuses prose, other routes, queries and fragments", () => {
    for (const text of [
      "",
      "see WEB-42",
      "https://app.test/acme/issues/42",
      "https://app.test/acme/browse/WEB-42?x=1",
      "https://app.test/acme/browse/WEB-42#top",
      "WEB-0",
      "WEB-",
      "not a url https://[",
    ]) {
      expect(parseEntityAddress(text)).toBeNull();
    }
  });

  it("refuses a key too large to be a safe sequence", () => {
    expect(parseEntityAddress("WEB-9007199254740992")).toBeNull();
  });
});
