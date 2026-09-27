/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { Request, Response } from "express";
import { describe, it, expect, vi } from "vitest";
import * as Y from "yjs";
import {
  convertBase64StringToBinaryData,
  convertHTMLDocumentToAllFormats,
  getAllDocumentFormatsFromDocumentEditorBinaryData,
} from "@pi-dash/editor";
import { DocumentController } from "@/controllers/document.controller";

const V1_HTML = "<p>First paragraph</p><p>Second paragraph</p>";
const V2_HTML = "<p>First paragraph</p><p>Second paragraph, edited by an agent</p><p>Third paragraph</p>";

/** What the editor serialises for the given HTML (adds e.g. `class="editor-paragraph-block"`). */
const canonical = (html: string, variant: "rich" | "document" = "document") =>
  convertHTMLDocumentToAllFormats({ document_html: html, variant }).description_html;

const decode = (base64: string) => new Uint8Array(convertBase64StringToBinaryData(base64));

/** HTML the document editor would serialise for the `default` fragment of a Y.Doc. */
const docHTML = (doc: Y.Doc) =>
  getAllDocumentFormatsFromDocumentEditorBinaryData(Y.encodeStateAsUpdate(doc), false).contentHTML;
const docTitle = (binary: Uint8Array) => getAllDocumentFormatsFromDocumentEditorBinaryData(binary, true).titleHTML;
const docFromBinary = (binary: Uint8Array) => {
  const doc = new Y.Doc();
  Y.applyUpdate(doc, binary);
  return doc;
};

describe("convertHTMLDocumentToAllFormats", () => {
  it.each(["rich", "document"] as const)("keeps a code block's language when parsed server-side (%s)", (variant) => {
    const result = convertHTMLDocumentToAllFormats({
      document_html: '<pre><code class="language-python">print(1)</code></pre>',
      variant,
    });

    expect(result.description_json).toMatchObject({
      content: [{ type: "codeBlock", attrs: { language: "python" } }],
    });
    expect(result.description_html).toContain('class="language-python"');
  });

  it.each(["rich", "document"] as const)("without a base binary keeps the existing output (%s)", (variant) => {
    const result = convertHTMLDocumentToAllFormats({ document_html: V1_HTML, variant });

    expect(new Set(Object.keys(result))).toEqual(
      new Set(["description_binary", "description_html", "description_json"])
    );
    expect(result.description_html).toContain("<p");
    expect(result.description_html).toContain("Second paragraph</p>");
    expect(result.description_json).toMatchObject({ type: "doc" });
    // passing explicit nulls is the same as omitting them (binaries differ only by the random Yjs clientID)
    const withNulls = convertHTMLDocumentToAllFormats({
      document_html: V1_HTML,
      variant,
      base_binary: null,
      title: null,
    });
    expect(withNulls.description_html).toBe(result.description_html);
    expect(withNulls.description_json).toEqual(result.description_json);
    // the doc has no title fragment content
    expect(docFromBinary(decode(result.description_binary)).getXmlFragment("title").length).toBe(0);
  });

  it.each(["rich", "document"] as const)(
    "with a base binary applies the new content as a diff that preserves lineage (%s)",
    (variant) => {
      const v1 = convertHTMLDocumentToAllFormats({ document_html: V1_HTML, variant });
      const v1Binary = decode(v1.description_binary);

      const v2 = convertHTMLDocumentToAllFormats({ document_html: V2_HTML, variant, base_binary: v1Binary });
      expect(v2.description_html).toBe(canonical(V2_HTML, variant));

      // Simulate a browser that has v1 cached in IndexedDB and syncs with the server's v2 doc.
      const cachedClient = docFromBinary(v1Binary);
      Y.applyUpdate(cachedClient, decode(v2.description_binary));
      expect(docHTML(cachedClient)).toBe(canonical(V2_HTML, variant));
      expect(cachedClient.getXmlFragment("default").length).toBe(3);

      // The v2 update also contains the full v1 history (it is a descendant of v1).
      const v1StateVector = Y.encodeStateVectorFromUpdate(v1Binary);
      const v2StateVector = Y.decodeStateVector(Y.encodeStateVectorFromUpdate(decode(v2.description_binary)));
      for (const [client, clock] of Y.decodeStateVector(v1StateVector)) {
        expect(v2StateVector.get(client)).toBeGreaterThanOrEqual(clock);
      }
    }
  );

  it("documents the bug: a from-scratch binary merged into a cached copy duplicates content", () => {
    const v1Binary = decode(
      convertHTMLDocumentToAllFormats({ document_html: V1_HTML, variant: "document" }).description_binary
    );
    const scratchV2 = convertHTMLDocumentToAllFormats({ document_html: V2_HTML, variant: "document" });

    const cachedClient = docFromBinary(v1Binary);
    Y.applyUpdate(cachedClient, decode(scratchV2.description_binary));

    // Two unrelated histories are merged: both v1's 2 blocks and v2's 3 blocks survive.
    expect(cachedClient.getXmlFragment("default").length).toBe(5);
    const merged = docHTML(cachedClient);
    expect(merged).not.toBe(canonical(V2_HTML));
    expect(merged.split("First paragraph").length - 1).toBe(2);
  });

  it("writes the title fragment on a fresh doc and replaces it on a base doc", () => {
    const v1 = convertHTMLDocumentToAllFormats({ document_html: V1_HTML, variant: "document", title: "Old title" });
    const v1Binary = decode(v1.description_binary);
    expect(v1.description_html).toBe(canonical(V1_HTML));
    expect(docTitle(v1Binary)).toBe("Old title");

    const v2 = convertHTMLDocumentToAllFormats({
      document_html: V2_HTML,
      variant: "document",
      base_binary: v1Binary,
      title: "New title",
    });
    const v2Binary = decode(v2.description_binary);
    expect(v2.description_html).toBe(canonical(V2_HTML));
    expect(docTitle(v2Binary)).toBe("New title");
    // a single level-1 heading, not a second one appended
    const titleFragment = docFromBinary(v2Binary).getXmlFragment("title");
    expect(titleFragment.length).toBe(1);
    expect(titleFragment.toString()).toBe('<heading level="1">New title</heading>');

    // a client holding the v1 cache converges on the new title too
    const cachedClient = docFromBinary(v1Binary);
    Y.applyUpdate(cachedClient, v2Binary);
    expect(docTitle(Y.encodeStateAsUpdate(cachedClient))).toBe("New title");

    // omitting the title leaves the base's title fragment untouched
    const v3 = convertHTMLDocumentToAllFormats({ document_html: V1_HTML, variant: "document", base_binary: v2Binary });
    expect(docTitle(decode(v3.description_binary))).toBe("New title");
  });
});

describe("DocumentController.convertDocument", () => {
  const call = async (body: unknown) => {
    const res = { status: vi.fn(), json: vi.fn() };
    res.status.mockReturnValue(res);
    res.json.mockReturnValue(res);
    await new DocumentController().convertDocument({ body } as Request, res as unknown as Response);
    return { status: res.status.mock.calls[0][0] as number, body: res.json.mock.calls[0][0] };
  };

  it("returns json, binary and html", async () => {
    const { status, body } = await call({ description_html: V1_HTML, variant: "document" });
    expect(status).toBe(200);
    expect(new Set(Object.keys(body))).toEqual(new Set(["description_binary", "description_html", "description_json"]));
    expect(body.description_html).toBe(canonical(V1_HTML));
  });

  it("applies description_binary and title", async () => {
    const v1 = await call({ description_html: V1_HTML, variant: "document", title: "Old" });
    const v2 = await call({
      description_html: V2_HTML,
      variant: "document",
      description_binary: v1.body.description_binary,
      title: "New",
    });
    expect(v2.status).toBe(200);
    expect(v2.body.description_html).toBe(canonical(V2_HTML));
    const cachedClient = docFromBinary(decode(v1.body.description_binary));
    Y.applyUpdate(cachedClient, decode(v2.body.description_binary));
    expect(docHTML(cachedClient)).toBe(canonical(V2_HTML));
    expect(docTitle(Y.encodeStateAsUpdate(cachedClient))).toBe("New");
  });

  it.each(["not base64!!", "abc", "aGVsbG8gd29ybGQ="])("rejects an invalid description_binary %j", async (value) => {
    const { status, body } = await call({ description_html: V1_HTML, variant: "document", description_binary: value });
    expect(status).toBe(400);
    expect(body.context.validationErrors[0].path).toBe("description_binary");
  });
});
