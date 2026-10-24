// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  isSpaceLandingTarget,
  parseSpaceLandingSearch,
  resolveSpaceReturnTarget,
  validateSpaceReturnPath,
} from "./returnPath.js";

describe("validateSpaceReturnPath", () => {
  it("keeps subtree-relative board positions with their query", () => {
    expect(validateSpaceReturnPath("/issues/anchor-1")).toBe("/issues/anchor-1");
    expect(validateSpaceReturnPath("/issues/anchor-1/")).toBe("/issues/anchor-1/");
    expect(validateSpaceReturnPath("/issues/anchor-1?board=kanban&peekId=issue-1")).toBe(
      "/issues/anchor-1?board=kanban&peekId=issue-1"
    );
    expect(validateSpaceReturnPath("/")).toBe("/");
  });

  it("drops the oracle's hostile return addresses", () => {
    for (const hostile of [
      "https://evil.example/issues/x",
      "//evil.example/issues/x",
      "javascript:alert(1)",
      "\\\\evil\\path",
      "issues/x",
    ]) {
      expect(validateSpaceReturnPath(hostile)).toBeNull();
    }
  });

  it("rejects non-strings, empties and backslashes", () => {
    expect(validateSpaceReturnPath(undefined)).toBeNull();
    expect(validateSpaceReturnPath(null)).toBeNull();
    expect(validateSpaceReturnPath(42)).toBeNull();
    expect(validateSpaceReturnPath({})).toBeNull();
    expect(validateSpaceReturnPath("")).toBeNull();
    expect(validateSpaceReturnPath("/\\evil\\path")).toBeNull();
  });

  it("rejects authorities smuggled through encoding", () => {
    expect(validateSpaceReturnPath("/%2f%2fevil.example/x")).toBeNull();
    expect(validateSpaceReturnPath("/%2F%2Fevil.example/x")).toBeNull();
    expect(validateSpaceReturnPath("/%5cevil")).toBeNull();
  });

  it("rejects decoded whitespace and control characters", () => {
    expect(validateSpaceReturnPath("/issues/a b")).toBeNull();
    expect(validateSpaceReturnPath("/issues/a%20b")).toBeNull();
    expect(validateSpaceReturnPath("/%09/evil")).toBeNull();
    expect(validateSpaceReturnPath("/%00/x")).toBeNull();
    expect(validateSpaceReturnPath("/%1f/x")).toBeNull();
    expect(validateSpaceReturnPath("/%7f/x")).toBeNull();
  });

  it("rejects active-content payloads in any casing or encoding", () => {
    expect(validateSpaceReturnPath("/javascript:alert(1)")).toBeNull();
    expect(validateSpaceReturnPath("/DATA:text/plain,x")).toBeNull();
    expect(validateSpaceReturnPath("/VbScRiPt:msgbox(1)")).toBeNull();
    expect(validateSpaceReturnPath("/%6aavascript:alert(1)")).toBeNull();
    expect(validateSpaceReturnPath("/issues/a?board=javascript:x")).toBeNull();
  });

  it("fails closed on malformed or over-encoded values", () => {
    expect(validateSpaceReturnPath("/100%/x")).toBeNull();
    expect(validateSpaceReturnPath("/%E0%A4%A")).toBeNull();
    expect(validateSpaceReturnPath("/%25252525")).toBeNull();
  });

  it("decodes multi-round values that settle", () => {
    expect(validateSpaceReturnPath("/%41")).toBe("/%41");
    expect(validateSpaceReturnPath("/%2541")).toBe("/%2541");
  });

  it("keeps `@` in path data: without a scheme it stays same-origin", () => {
    expect(validateSpaceReturnPath("/issues/a@b")).toBe("/issues/a@b");
    expect(validateSpaceReturnPath("/issues/a%40b")).toBe("/issues/a%40b");
  });
});

describe("resolveSpaceReturnTarget", () => {
  it("re-attaches the mount and keeps the query decoded", () => {
    expect(resolveSpaceReturnTarget("/issues/anchor-1/")).toEqual({ to: "/spaces/issues/anchor-1/", search: {} });
    expect(resolveSpaceReturnTarget("/issues/a?board=kanban&peekId=x")).toEqual({
      to: "/spaces/issues/a",
      search: { board: "kanban", peekId: "x" },
    });
    expect(resolveSpaceReturnTarget("/")).toEqual({ to: "/spaces/", search: {} });
  });

  it("drops fragments and lets the last duplicate win", () => {
    expect(resolveSpaceReturnTarget("/issues/a#frag")).toEqual({ to: "/spaces/issues/a", search: {} });
    expect(resolveSpaceReturnTarget("/issues/a?board=kanban#frag")).toEqual({
      to: "/spaces/issues/a",
      search: { board: "kanban" },
    });
    expect(resolveSpaceReturnTarget("/issues/a?board=list&board=kanban")).toEqual({
      to: "/spaces/issues/a",
      search: { board: "kanban" },
    });
    expect(resolveSpaceReturnTarget("/issues/a?board=")).toEqual({ to: "/spaces/issues/a", search: { board: "" } });
  });

  it("recognises the landing document itself", () => {
    expect(isSpaceLandingTarget({ to: "/spaces/", search: {} })).toBe(true);
    expect(isSpaceLandingTarget({ to: "/spaces", search: {} })).toBe(true);
    expect(isSpaceLandingTarget({ to: "/spaces/issues/a", search: {} })).toBe(false);
  });
});

describe("parseSpaceLandingSearch", () => {
  it("passes present params through untouched", () => {
    expect(
      parseSpaceLandingSearch({ next_path: "/issues/a?board=kanban", email: "a@b.test", error_code: "5092" })
    ).toEqual({
      nextPath: "/issues/a?board=kanban",
      email: "a@b.test",
      errorCode: "5092",
    });
  });

  it("reads absent, empty and mistyped params as missing", () => {
    expect(parseSpaceLandingSearch({})).toEqual({});
    expect(parseSpaceLandingSearch({ next_path: "", email: "", error_code: "" })).toEqual({});
    expect(parseSpaceLandingSearch({ next_path: 42, email: null, error_code: ["x"] })).toEqual({});
    expect(parseSpaceLandingSearch({ error_code: 50.5 })).toEqual({});
  });

  it("normalizes router-decoded numeric error codes", () => {
    expect(parseSpaceLandingSearch({ error_code: 5075 })).toEqual({ errorCode: "5075" });
  });
});
