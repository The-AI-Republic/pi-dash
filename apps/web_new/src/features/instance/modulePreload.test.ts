// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { issuesScreenPreloadTag, withIssuesScreenPreload } from "./modulePreload.js";

describe("issues screen modulepreload", () => {
  it("builds a modulepreload link for the emitted asset", () => {
    expect(issuesScreenPreloadTag("assets/IssuesListContent-abc.js")).toBe(
      '<link rel="modulepreload" crossorigin href="/assets/IssuesListContent-abc.js">'
    );
  });

  it("adds the preload just before the head closes", () => {
    const html = "<html>\n  <head>\n    <title>x</title>\n  </head>\n  <body></body>\n</html>\n";
    expect(withIssuesScreenPreload(html, "assets/screen.js")).toBe(
      '<html>\n  <head>\n    <title>x</title>\n  <link rel="modulepreload" crossorigin href="/assets/screen.js">\n</head>\n  <body></body>\n</html>\n'
    );
  });

  it("does not duplicate an existing preload", () => {
    const html = '<head><link rel="modulepreload" crossorigin href="/assets/screen.js"></head>';
    expect(withIssuesScreenPreload(html, "assets/screen.js")).toBe(html);
  });
});
