// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// DOM coverage for the exports picker option-name reader serialized into
// page.evaluate (NEWFRONT-772, EXP-003): aria-label wins, aria-hidden mark
// and check spans are stripped, whitespace collapses.
import { describe, expect, it } from "vitest";

import { exportsProjectOptionName } from "./web-new.js";

function option(html: string): Element {
  const host = document.createElement("div");
  host.innerHTML = html;
  const found = host.firstElementChild;
  if (found === null) throw new Error("fixture produced no element");
  return found;
}

describe("exportsProjectOptionName", () => {
  it("prefers aria-label over rendered text", () => {
    const el = option('<button role="option" aria-label="Exp Alpha mvtag">E<span>Exp Alpha mvtag</span></button>');
    expect(exportsProjectOptionName(el)).toBe("Exp Alpha mvtag");
  });

  it("strips the aria-hidden mark initial from the name", () => {
    const el = option('<button role="option"><span aria-hidden="true">E</span><span>Exp Alpha mvtag</span></button>');
    expect(exportsProjectOptionName(el)).toBe("Exp Alpha mvtag");
  });

  it("strips the selected check alongside the mark", () => {
    const el = option(
      '<button role="option" aria-selected="true"><span aria-hidden="true">E</span>' +
        "<span>Exp Alpha mvtag</span>" +
        '<span aria-hidden="true"><svg></svg></span></button>'
    );
    expect(exportsProjectOptionName(el)).toBe("Exp Alpha mvtag");
  });

  it("collapses whitespace and ignores a blank aria-label", () => {
    const el = option(
      '<button role="option" aria-label="  "><span aria-hidden="true">E</span><span>  Exp\n  Alpha   mvtag </span></button>'
    );
    expect(exportsProjectOptionName(el)).toBe("Exp Alpha mvtag");
  });

  it("resolves an empty option to the empty string", () => {
    const el = option('<button role="option"><span aria-hidden="true">E</span></button>');
    expect(exportsProjectOptionName(el)).toBe("");
  });
});
