// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  descriptionAssetSrc,
  hasStoredDescriptionContent,
  isStoredHtmlEmpty,
  isStoredHtmlEmptyIgnoring,
  mentionDisplayName,
  prepareEditorHtml,
  renderableHtml,
  sanitizeHtml,
  STORED_CONTENT_TAGS,
  storedImageSrc,
  storedMentionOf,
  storedTextContent,
} from "./sanitize.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("sanitizeHtml", () => {
  it("keeps formatting while stripping scripts and handlers", () => {
    const clean = sanitizeHtml(
      '<p>Hello <strong>world</strong> <a href="https://example.com">link</a></p><script>alert(1)</script><img src=x onerror=alert(2)>'
    );
    expect(clean).toContain("<strong>world</strong>");
    expect(clean).toContain('href="https://example.com"');
    expect(clean).not.toContain("<script>");
    expect(clean).not.toContain("onerror");
  });

  it("keeps a stored link's target on the anchor and nowhere else", () => {
    const clean = sanitizeHtml(
      '<p><a href="https://example.com" target="_blank" rel="noopener noreferrer nofollow">out</a>' +
        '<span target="_blank">inline</span></p><img src="x" target="_blank">'
    );
    expect(clean).toContain('<a href="https://example.com" target="_blank" rel="noopener noreferrer nofollow">');
    expect(clean).toContain("<span>inline</span>");
    expect(clean).toContain('<img src="x">');
    expect(sanitizeHtml('<a href="javascript:alert(1)" target="_blank">x</a>')).not.toContain("javascript:");
  });

  it("drops javascript: links", () => {
    expect(sanitizeHtml('<a href="javascript:alert(1)">x</a>')).not.toContain("javascript:");
  });

  it("passes empty input through untouched", () => {
    expect(sanitizeHtml("")).toBe("");
  });

  it("keeps the editor's stored mention and image elements (ISS-127)", () => {
    const clean = sanitizeHtml(
      '<p>hi <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component></p>' +
        '<p><image-component src="https://cdn.test/a.png" alt="shot"></image-component></p>'
    );
    expect(clean).toContain("<mention-component");
    expect(clean).toContain('entity_identifier="u-1"');
    expect(clean).toContain('data-name="Ada"');
    expect(clean).toContain("<image-component");
    expect(clean).toContain('src="https://cdn.test/a.png"');
  });

  it("keeps the contract's shared content elements", () => {
    expect(STORED_CONTENT_TAGS).toContain("mention-component");
    expect(STORED_CONTENT_TAGS).toContain("image-component");
    const clean = sanitizeHtml('<label for="x">a</label><input type="checkbox" checked="checked">');
    expect(clean).toContain("<label");
    expect(clean).toContain("<input");
  });

  it("still strips scripts and handlers from stored content elements", () => {
    const clean = sanitizeHtml(
      '<mention-component id="u-1" onclick="steal()"></mention-component><image-component src="javascript:alert(1)" onerror="x()"></image-component>'
    );
    expect(clean).not.toContain("onclick");
    expect(clean).not.toContain("onerror");
    expect(clean).not.toContain("javascript:");
  });
});

describe("storedMentionOf", () => {
  function element(html: string): Element {
    const document = new DOMParser().parseFromString(html, "text/html");
    const found = document.body.firstElementChild;
    if (found === null) throw new Error("no element");
    return found;
  }

  it("reads the identity and stored name", () => {
    expect(
      storedMentionOf(
        element(
          '<mention-component id="u-1" entity_identifier="e-1" entity_name="user_mention" data-name="Ada"></mention-component>'
        )
      )
    ).toEqual({ id: "u-1", entityIdentifier: "e-1", entityName: "user_mention", name: "Ada" });
  });

  it("falls back to the id and the user entity on older bodies", () => {
    expect(storedMentionOf(element('<mention-component id="u-2"></mention-component>'))).toEqual({
      id: "u-2",
      entityIdentifier: "u-2",
      entityName: "user_mention",
      name: null,
    });
  });

  it("rejects a mention with no identity", () => {
    expect(storedMentionOf(element("<mention-component></mention-component>"))).toBeNull();
  });
});

describe("mentionDisplayName", () => {
  it("prefers the stored name and falls back to the identifier", () => {
    expect(mentionDisplayName({ id: "u-1", entityIdentifier: "u-1", entityName: "user_mention", name: "Ada" })).toBe(
      "Ada"
    );
    expect(mentionDisplayName({ id: "u-1", entityIdentifier: "u-1", entityName: "user_mention", name: "  " })).toBe(
      "u-1"
    );
  });
});

describe("renderableHtml", () => {
  it("paints a stored mention as a chip naming the person", () => {
    const html = renderableHtml(
      '<p>hi <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component></p>'
    );
    expect(html).toContain('class="mention-chip"');
    expect(html).toContain("@Ada");
    expect(html).not.toContain("mention-component");
  });

  it("falls back to the identifier when the body holds no name", () => {
    expect(renderableHtml('<p><mention-component id="u-9"></mention-component></p>')).toContain("@u-9");
  });

  it("links a person mention to the member profile when the workspace is known (CMT-020)", () => {
    const html = renderableHtml(
      '<p>hi <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component></p>',
      undefined,
      "acme"
    );
    expect(html).toContain('<a href="/acme/profile/u-1" data-mention-id="u-1" class="mention-chip">@Ada</a>');
    expect(html).not.toContain("mention-component");
  });

  it("keeps other entity kinds as plain chips even with a workspace (CMT-020)", () => {
    const html = renderableHtml(
      '<p>see <mention-component id="w-3" entity_identifier="w-3" entity_name="work_item" data-name="Login"></mention-component></p>',
      undefined,
      "acme"
    );
    expect(html).toContain('class="mention-chip"');
    expect(html).toContain("@Login");
    expect(html).not.toContain("<a");
  });

  it("keeps a plain chip when the workspace slug is empty (CMT-020)", () => {
    const html = renderableHtml(
      '<p>hi <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component></p>',
      undefined,
      ""
    );
    expect(html).toContain('class="mention-chip"');
    expect(html).not.toContain("<a");
  });

  it("drops a mention with no identity rather than leaving a gap", () => {
    expect(renderableHtml("<p>a<mention-component></mention-component>b</p>")).toBe("<p>ab</p>");
  });

  it("turns a stored image component into an image", () => {
    const html = renderableHtml('<image-component src="https://cdn.test/a.png" alt="shot"></image-component>');
    expect(html).toContain('<img src="https://cdn.test/a.png" alt="shot">');
    expect(html).not.toContain("image-component");
  });

  it("drops an image component with no source", () => {
    expect(renderableHtml("<p>a<image-component></image-component></p>")).toBe("<p>a</p>");
  });

  it("keeps ordinary markup and still sanitizes", () => {
    const html = renderableHtml("<p>ok <strong>x</strong></p><script>alert(1)</script>");
    expect(html).toContain("<strong>x</strong>");
    expect(html).not.toContain("<script>");
  });

  it("returns the sanitized markup where no DOM parser exists", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(renderableHtml("<p>hi</p>")).toBe("<p>hi</p>");
  });

  it("resolves a stored asset id through the given scope (ISS-127)", () => {
    const html = renderableHtml(
      '<image-component src="35e8b958-6ee5-43ce-ae56-fb0e776f421e" status="uploaded"></image-component>',
      (src) => descriptionAssetSrc(src, { workspaceSlug: "acme", projectId: "p-1", apiBaseUrl: "https://api.test" })
    );
    expect(html).toContain(
      'src="https://api.test/api/assets/v2/workspaces/acme/projects/p-1/35e8b958-6ee5-43ce-ae56-fb0e776f421e/"'
    );
  });
});

describe("descriptionAssetSrc", () => {
  it("joins a bare asset id to the project's asset path", () => {
    expect(descriptionAssetSrc("asset-1", { workspaceSlug: "acme", projectId: "p-1" })).toBe(
      "/api/assets/v2/workspaces/acme/projects/p-1/asset-1/"
    );
  });

  it("uses the workspace path when the row holds no project", () => {
    expect(descriptionAssetSrc("asset-1", { workspaceSlug: "acme", projectId: null })).toBe(
      "/api/assets/v2/workspaces/acme/asset-1/"
    );
  });

  it("leaves absolute, data, blob and rooted sources alone", () => {
    const scope = { workspaceSlug: "acme", projectId: "p-1" };
    for (const src of ["https://cdn.test/a.png", "data:image/png;base64,AA", "blob:x", "/media/a.png"]) {
      expect(descriptionAssetSrc(src, scope)).toBe(src);
    }
  });

  it("returns the source untouched without a scope or for empty input", () => {
    expect(descriptionAssetSrc("asset-1", undefined)).toBe("asset-1");
    expect(descriptionAssetSrc("", { workspaceSlug: "acme", projectId: "p-1" })).toBe("");
  });

  it("prefixes the API origin when one is given", () => {
    expect(
      descriptionAssetSrc("asset-1", { workspaceSlug: "acme", projectId: "p-1", apiBaseUrl: "https://api.test/" })
    ).toBe("https://api.test/api/assets/v2/workspaces/acme/projects/p-1/asset-1/");
  });
});

describe("prepareEditorHtml", () => {
  it("paints the resolved source and keeps the stored id", () => {
    const html = prepareEditorHtml(
      '<p><image-component src="asset-1" width="200"></image-component></p>',
      (src) => `/assets/${src}`
    );
    expect(html).toContain('src="/assets/asset-1"');
    expect(html).toContain('data-stored-src="asset-1"');
    expect(html).toContain('width="200"');
  });

  it("leaves absolute sources and other markup alone", () => {
    const stored = '<p>a</p><image-component src="https://cdn.test/a.png"></image-component>';
    expect(prepareEditorHtml(stored, (src) => `/assets/${src}`)).toContain('src="https://cdn.test/a.png"');
    expect(prepareEditorHtml(stored, (src) => `/assets/${src}`)).not.toContain("data-stored-src");
  });

  it("passes the input through without a resolver or DOM parser", () => {
    const stored = '<image-component src="asset-1"></image-component>';
    expect(prepareEditorHtml(stored, undefined)).toBe(stored);
    vi.stubGlobal("DOMParser", undefined);
    expect(prepareEditorHtml(stored, (src) => `/assets/${src}`)).toBe(stored);
  });
});

describe("storedImageSrc", () => {
  it("prefers the stored id carried beside a resolved source", () => {
    const document = new DOMParser().parseFromString(
      '<image-component src="/resolved" data-stored-src="asset-1"></image-component>',
      "text/html"
    );
    expect(storedImageSrc(document.body.firstElementChild!)).toBe("asset-1");
  });

  it("falls back to the source and to an empty string", () => {
    const document = new DOMParser().parseFromString(
      "<image-component src='a'></image-component><image-component></image-component>",
      "text/html"
    );
    expect(storedImageSrc(document.body.firstElementChild!)).toBe("a");
    expect(storedImageSrc(document.body.lastElementChild!)).toBe("");
  });
});

describe("storedTextContent", () => {
  it("reads the visible text out of stored markup", () => {
    expect(storedTextContent("<h2>Title</h2><p>one <em>two</em></p>")).toBe("Titleone two");
  });

  it("returns an empty string for empty markup", () => {
    expect(storedTextContent("")).toBe("");
    expect(storedTextContent("<p></p>")).toBe("");
  });

  it("reads text from documents whose body holds no text", () => {
    const parse = vi.fn(() => ({ body: { textContent: null } }));
    vi.stubGlobal("DOMParser", function StubParser() {
      return { parseFromString: parse };
    });
    expect(storedTextContent("<p>hi</p>")).toBe("");
    expect(parse).toHaveBeenCalledWith("<p>hi</p>", "text/html");
  });

  it("returns an empty string where no DOM parser exists", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(storedTextContent("<p>hi</p>")).toBe("");
  });
});

describe("isStoredHtmlEmpty", () => {
  it("treats whitespace-only markup as empty", () => {
    expect(isStoredHtmlEmpty("")).toBe(true);
    expect(isStoredHtmlEmpty("<p></p>")).toBe(true);
    expect(isStoredHtmlEmpty("<p>   </p>")).toBe(true);
  });

  it("treats markup with text as non-empty", () => {
    expect(isStoredHtmlEmpty("<p>hi</p>")).toBe(false);
  });
});

describe("isStoredHtmlEmptyIgnoring", () => {
  it("keeps the word rule", () => {
    expect(isStoredHtmlEmptyIgnoring("<p></p>", ["img"])).toBe(true);
    expect(isStoredHtmlEmptyIgnoring("<p>hi</p>", ["img"])).toBe(false);
  });

  it("treats an allowed tag as content, in any letter case", () => {
    expect(isStoredHtmlEmptyIgnoring('<p><img src="/a.png"></p>', ["img"])).toBe(false);
    expect(isStoredHtmlEmptyIgnoring('<p><IMG src="/a.png"></p>', ["IMG"])).toBe(false);
    expect(isStoredHtmlEmptyIgnoring("<p><span> </span></p>", ["img"])).toBe(true);
    expect(isStoredHtmlEmptyIgnoring('<p><img src="/a.png"></p>', [])).toBe(true);
  });

  it("reports empty where no DOM parser exists", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(isStoredHtmlEmptyIgnoring('<p><img src="/a.png"></p>', ["img"])).toBe(true);
  });
});

describe("hasStoredDescriptionContent", () => {
  it("counts words", () => {
    expect(hasStoredDescriptionContent("<p>Words</p>")).toBe(true);
  });

  it("counts an image or a mention with no words (ISS-127)", () => {
    expect(hasStoredDescriptionContent('<p><img src="/a.png"></p>')).toBe(true);
    expect(hasStoredDescriptionContent('<p><mention-component id="u-1"></mention-component></p>')).toBe(true);
    expect(hasStoredDescriptionContent('<p><image-component src="/a.png"></image-component></p>')).toBe(true);
  });

  it("treats the editor's empty paragraph and blank markup as nothing", () => {
    expect(hasStoredDescriptionContent("")).toBe(false);
    expect(hasStoredDescriptionContent("<p></p>")).toBe(false);
    expect(hasStoredDescriptionContent("<p>   </p>")).toBe(false);
    expect(hasStoredDescriptionContent("<p><span> </span></p>")).toBe(false);
  });

  it("reports no content where no DOM parser exists", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(hasStoredDescriptionContent('<p><img src="/a.png"></p>')).toBe(false);
  });
});
