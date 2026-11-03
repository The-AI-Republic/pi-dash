/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Buffer } from "buffer";
import type { Extensions, JSONContent } from "@tiptap/core";
import { getSchema } from "@tiptap/core";
import { generateJSON } from "@tiptap/html";
import { DOMSerializer, Node as ProseMirrorNode } from "@tiptap/pm/model";
import { prosemirrorJSONToYDoc, prosemirrorJSONToYXmlFragment, yXmlFragmentToProseMirrorRootNode } from "y-prosemirror";
import * as Y from "yjs";
// extensions
import type { TDocumentPayload } from "@pi-dash/types";
import {
  CoreEditorExtensionsWithoutProps,
  DocumentEditorExtensionsWithoutProps,
} from "@/extensions/core-without-props";
import { TitleExtensions } from "@/extensions/title-extension";
import { sanitizeHTML } from "@pi-dash/utils";

// editor extension configs
const RICH_TEXT_EDITOR_EXTENSIONS = CoreEditorExtensionsWithoutProps;
const DOCUMENT_EDITOR_EXTENSIONS = [...CoreEditorExtensionsWithoutProps, ...DocumentEditorExtensionsWithoutProps];
export const TITLE_EDITOR_EXTENSIONS: Extensions = TitleExtensions;
// editor schemas
const richTextEditorSchema = getSchema(RICH_TEXT_EDITOR_EXTENSIONS);
const documentEditorSchema = getSchema(DOCUMENT_EDITOR_EXTENSIONS);
const titleEditorSchema = getSchema(TITLE_EDITOR_EXTENSIONS);

/**
 * HTML void elements: rendered without a closing tag, matching the previous server renderer's output.
 */
const VOID_ELEMENTS = new Set([
  "area",
  "base",
  "br",
  "col",
  "embed",
  "hr",
  "img",
  "input",
  "keygen",
  "link",
  "meta",
  "param",
  "source",
  "track",
  "wbr",
  "command",
]);

/**
 * Escapes text and attribute values for HTML output, matching the previous server renderer's output.
 */
const escapeHTML = (value: string): string =>
  value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/'/g, "&apos;")
    .replace(/"/g, "&quot;")
    .replace(/\u00a0/g, "&nbsp;")
    .replace(/\u00ad/g, "&shy;");

type SimpleChild = SimpleElement | SimpleText;

class SimpleText {
  readonly nodeType = 3;
  constructor(readonly text: string) {}
}

/**
 * Minimal style declaration. The `cssText` setter keeps the declarations a browser would keep:
 * it drops empty declarations and ones whose value is `null` or `none` (the table specs
 * interpolate unset colours into the style string, and browsers drop those as invalid), skips the
 * attribute when nothing remains, and otherwise serialises like a browser (`"; "`-joined with a
 * trailing semicolon).
 */
class SimpleStyle {
  constructor(private readonly element: SimpleElement) {}
  set cssText(value: string) {
    const declarations = value
      .split(";")
      .map((part) => part.trim())
      .filter((part) => {
        const separator = part.indexOf(":");
        if (separator === -1) return false;
        const declarationValue = part
          .slice(separator + 1)
          .trim()
          .toLowerCase();
        return declarationValue !== "null" && declarationValue !== "none";
      });
    if (declarations.length > 0) {
      this.element.attributes["style"] = `${declarations.join("; ")};`;
    }
  }
}

class SimpleElement {
  readonly nodeType = 1;
  readonly attributes: Record<string, unknown> = {};
  readonly children: SimpleChild[] = [];
  readonly style: SimpleStyle;
  constructor(readonly tagName: string) {
    this.style = new SimpleStyle(this);
  }
  setAttribute(name: string, value: unknown): void {
    this.attributes[name] = value;
  }
  appendChild<T extends SimpleChild>(child: T): T {
    this.children.push(child);
    return child;
  }
}

class SimpleFragment {
  readonly nodeType = 11;
  readonly children: SimpleChild[] = [];
  appendChild<T extends SimpleChild>(child: T): T {
    this.children.push(child);
    return child;
  }
}

/**
 * Minimal document for ProseMirror's `DOMSerializer` on the server. It implements only the surface
 * the serializer touches (`createElement`, `createTextNode`, `createDocumentFragment`); unlike the
 * DOM `@tiptap/html` renders with, its elements honour the `style.cssText` assignment.
 */
class SimpleDocument {
  createElement(tagName: string): SimpleElement {
    return new SimpleElement(tagName);
  }
  createTextNode(text: string): SimpleText {
    return new SimpleText(text);
  }
  createDocumentFragment(): SimpleFragment {
    return new SimpleFragment();
  }
}

const renderSimpleAttribute = (name: string, value: unknown): string => {
  if (value === true) return ` ${name}`;
  if (value === false) return "";
  return ` ${name}="${escapeHTML(String(value))}"`;
};

const renderSimpleChild = (child: SimpleChild): string => {
  if (child instanceof SimpleText) return escapeHTML(child.text);
  const attributes = Object.entries(child.attributes)
    .map(([name, value]) => renderSimpleAttribute(name, value))
    .join("");
  if (VOID_ELEMENTS.has(child.tagName)) return `<${child.tagName}${attributes}>`;
  const inner = child.children.map(renderSimpleChild).join("");
  return `<${child.tagName}${attributes}>${inner}</${child.tagName}>`;
};

const renderSimpleFragment = (fragment: SimpleFragment): string => fragment.children.map(renderSimpleChild).join("");

/**
 * @description serialises ProseMirror JSON to HTML on the server, preserving `style` attributes.
 * `@tiptap/html`'s `generateHTML` renders with a DOM whose element `style` object silently drops
 * ProseMirror's `style.cssText` assignment, so every schema-emitted `style` (text alignment, custom
 * colours, table cell colours) is lost from the stored HTML. This runs the same `DOMSerializer`
 * pass against {@link SimpleDocument} and renders with the same escaping, attribute and
 * void-element conventions as before; `style` values are serialised like a browser would.
 * @param {JSONContent} contentJSON
 * @param {typeof documentEditorSchema} schema
 * @returns {string}
 */
const prosemirrorJSONToHTML = (contentJSON: JSONContent, schema: typeof documentEditorSchema): string => {
  const contentNode = ProseMirrorNode.fromJSON(schema, contentJSON);
  const fragment = DOMSerializer.fromSchema(schema).serializeFragment(contentNode.content, {
    document: new SimpleDocument() as unknown as Document,
  });
  return renderSimpleFragment(fragment as unknown as SimpleFragment);
};

/**
 * @description apply updates to a doc and return the updated doc in binary format
 * @param {Uint8Array} document
 * @param {Uint8Array} updates
 * @returns {Uint8Array}
 */
export const applyUpdates = (document: Uint8Array, updates?: Uint8Array): Uint8Array => {
  const yDoc = new Y.Doc();
  Y.applyUpdate(yDoc, document);
  if (updates) {
    Y.applyUpdate(yDoc, updates);
  }

  const encodedDoc = Y.encodeStateAsUpdate(yDoc);
  return encodedDoc;
};

/**
 * @description this function encodes binary data to base64 string
 * @param {Uint8Array} document
 * @returns {string}
 */
export const convertBinaryDataToBase64String = (document: Uint8Array): string =>
  Buffer.from(document).toString("base64");

/**
 * @description this function decodes base64 string to binary data
 * @param {string} document
 * @returns {Buffer<ArrayBuffer>}
 */
export const convertBase64StringToBinaryData = (document: string): Buffer<ArrayBuffer> =>
  Buffer.from(document, "base64");

/**
 * @description this function generates the binary equivalent of html content for the rich text editor
 * @param {string} descriptionHTML
 * @returns {Uint8Array}
 */
export const getBinaryDataFromRichTextEditorHTMLString = (descriptionHTML: string): Uint8Array => {
  // convert HTML to JSON
  const contentJSON = generateJSON(descriptionHTML ?? "<p></p>", RICH_TEXT_EDITOR_EXTENSIONS);
  // convert JSON to Y.Doc format
  const transformedData = prosemirrorJSONToYDoc(richTextEditorSchema, contentJSON, "default");
  // convert Y.Doc to Uint8Array format
  const encodedData = Y.encodeStateAsUpdate(transformedData);
  return encodedData;
};

export const generateTitleProsemirrorJson = (text: string): JSONContent => {
  return {
    type: "doc",
    content: [
      {
        type: "heading",
        attrs: { level: 1 },
        ...(text
          ? {
              content: [
                {
                  type: "text",
                  text,
                },
              ],
            }
          : {}),
      },
    ],
  };
};

/**
 * @description this function generates the binary equivalent of html content for the document editor
 * @param {string} descriptionHTML - The HTML content to convert
 * @param {string} [title] - Optional title to append to the document
 * @returns {Uint8Array}
 */
export const getBinaryDataFromDocumentEditorHTMLString = (descriptionHTML: string, title?: string): Uint8Array => {
  // convert HTML to JSON
  const contentJSON = generateJSON(descriptionHTML ?? "<p></p>", DOCUMENT_EDITOR_EXTENSIONS);
  // convert JSON to Y.Doc format
  const transformedData = prosemirrorJSONToYDoc(documentEditorSchema, contentJSON, "default");

  // If title is provided, merge it into the document
  if (title != null) {
    const titleJSON = generateTitleProsemirrorJson(title);
    const titleField = prosemirrorJSONToYDoc(documentEditorSchema, titleJSON, "title");
    // Encode the title YDoc to updates and apply them to the main document
    const titleUpdates = Y.encodeStateAsUpdate(titleField);
    Y.applyUpdate(transformedData, titleUpdates);
  }

  // convert Y.Doc to Uint8Array format
  const encodedData = Y.encodeStateAsUpdate(transformedData);
  return encodedData;
};

/**
 * @description this function generates all document formats for the provided binary data for the rich text editor
 * @param {Uint8Array} description
 * @returns
 */
export const getAllDocumentFormatsFromRichTextEditorBinaryData = (
  description: Uint8Array
): {
  contentBinaryEncoded: string;
  contentJSON: object;
  contentHTML: string;
} => {
  // encode binary description data
  const base64Data = convertBinaryDataToBase64String(description);
  const yDoc = new Y.Doc();
  Y.applyUpdate(yDoc, description);
  // convert to JSON
  const type = yDoc.getXmlFragment("default");
  const contentJSON = yXmlFragmentToProseMirrorRootNode(type, richTextEditorSchema).toJSON();
  // convert to HTML
  const contentHTML = prosemirrorJSONToHTML(contentJSON, richTextEditorSchema);

  return {
    contentBinaryEncoded: base64Data,
    contentJSON,
    contentHTML,
  };
};

/**
 * @description this function generates all document formats for the provided binary data for the document editor
 * @param {Uint8Array} description
 * @returns
 */
export const getAllDocumentFormatsFromDocumentEditorBinaryData = (
  description: Uint8Array,
  updateTitle: boolean
): {
  contentBinaryEncoded: string;
  contentJSON: object;
  contentHTML: string;
  titleHTML?: string;
} => {
  // encode binary description data
  const base64Data = convertBinaryDataToBase64String(description);
  const yDoc = new Y.Doc();
  Y.applyUpdate(yDoc, description);
  // convert to JSON
  const type = yDoc.getXmlFragment("default");
  const contentJSON = yXmlFragmentToProseMirrorRootNode(type, documentEditorSchema).toJSON();
  // convert to HTML
  const contentHTML = prosemirrorJSONToHTML(contentJSON, documentEditorSchema);

  if (updateTitle) {
    const title = yDoc.getXmlFragment("title");
    const titleJSON = yXmlFragmentToProseMirrorRootNode(title, documentEditorSchema).toJSON();
    const titleHTML = extractTextFromHTML(prosemirrorJSONToHTML(titleJSON, documentEditorSchema));

    return {
      contentBinaryEncoded: base64Data,
      contentJSON,
      contentHTML,
      titleHTML,
    };
  } else {
    return {
      contentBinaryEncoded: base64Data,
      contentJSON,
      contentHTML,
    };
  }
};

type TConvertHTMLDocumentToAllFormatsArgs = {
  document_html: string;
  variant: "rich" | "document";
  /**
   * Optional current Yjs state of the document. When supplied, the new content is
   * applied as an in-place diff onto this doc's `default` fragment (preserving the
   * Yjs lineage) instead of building a brand-new Y.Doc from scratch.
   */
  base_binary?: Uint8Array | null;
  /**
   * Optional title. When supplied, the doc's `title` fragment is set to this text
   * (diffed in place when a base binary is given).
   */
  title?: string | null;
};

/**
 * @description Builds the Yjs binary for new HTML content, optionally on top of an existing doc.
 * With a base binary, the `default` (and, if a title is given, `title`) fragments are updated in
 * place via y-prosemirror's `updateYFragment`, so the result is a descendant of the base doc: a
 * client holding a cached copy of the base (e.g. y-indexeddb) can merge it without duplication.
 * @returns {Uint8Array} the full encoded state of the resulting doc
 */
const getBinaryDataFromHTMLString = (args: {
  document_html: string;
  schema: typeof documentEditorSchema;
  extensions: Extensions;
  base_binary?: Uint8Array | null;
  title?: string | null;
}): Uint8Array => {
  const { document_html, schema, extensions, base_binary, title } = args;
  const yDoc = new Y.Doc();
  if (base_binary && base_binary.length > 0) {
    Y.applyUpdate(yDoc, base_binary);
  }
  const contentJSON = generateJSON(document_html ?? "<p></p>", extensions);
  yDoc.transact(() => {
    prosemirrorJSONToYXmlFragment(schema, contentJSON, yDoc.getXmlFragment("default"));
    if (title != null) {
      prosemirrorJSONToYXmlFragment(
        titleEditorSchema,
        generateTitleProsemirrorJson(title),
        yDoc.getXmlFragment("title")
      );
    }
  });
  return Y.encodeStateAsUpdate(yDoc);
};

/**
 * @description Converts HTML content to all supported document formats (JSON, HTML, and binary)
 * @param {TConvertHTMLDocumentToAllFormatsArgs} args - Arguments containing HTML content and variant type
 * @param {string} args.document_html - The HTML content to convert
 * @param {"rich" | "document"} args.variant - The type of editor variant to use for conversion
 * @param {Uint8Array} [args.base_binary] - Optional existing Yjs state to apply the new content onto
 * @param {string} [args.title] - Optional title to write into the doc's `title` fragment
 * @returns {TDocumentPayload} Object containing the document in all supported formats. `description_html`
 *   is regenerated from the resulting doc (i.e. what the editor would serialise).
 * @throws {Error} If an invalid variant is provided
 */
export const convertHTMLDocumentToAllFormats = (args: TConvertHTMLDocumentToAllFormatsArgs): TDocumentPayload => {
  const { document_html, variant, base_binary, title } = args;
  const hasBase = !!base_binary && base_binary.length > 0;
  const hasTitle = title != null;

  let allFormats: TDocumentPayload;

  if (variant === "rich") {
    // Convert HTML to binary format for rich text editor
    const contentBinary =
      hasBase || hasTitle
        ? getBinaryDataFromHTMLString({
            document_html,
            schema: richTextEditorSchema,
            extensions: RICH_TEXT_EDITOR_EXTENSIONS,
            base_binary,
            title,
          })
        : getBinaryDataFromRichTextEditorHTMLString(document_html);
    // Generate all document formats from the binary data
    const { contentBinaryEncoded, contentHTML, contentJSON } =
      getAllDocumentFormatsFromRichTextEditorBinaryData(contentBinary);
    allFormats = {
      description_json: contentJSON,
      description_html: contentHTML,
      description_binary: contentBinaryEncoded,
    };
  } else if (variant === "document") {
    // Convert HTML to binary format for document editor
    const contentBinary =
      hasBase || hasTitle
        ? getBinaryDataFromHTMLString({
            document_html,
            schema: documentEditorSchema,
            extensions: DOCUMENT_EDITOR_EXTENSIONS,
            base_binary,
            title,
          })
        : getBinaryDataFromDocumentEditorHTMLString(document_html);
    // Generate all document formats from the binary data
    const { contentBinaryEncoded, contentHTML, contentJSON } = getAllDocumentFormatsFromDocumentEditorBinaryData(
      contentBinary,
      false
    );
    allFormats = {
      description_json: contentJSON,
      description_html: contentHTML,
      description_binary: contentBinaryEncoded,
    };
  } else {
    throw new Error(`Invalid variant provided: ${variant}`);
  }

  return allFormats;
};

export const extractTextFromHTML = (html: string): string => {
  // Use DOMPurify to safely extract text and remove all HTML tags
  // This is more secure than regex as it handles edge cases and prevents injection
  // Note: sanitizeHTML trims whitespace, which is acceptable for title extraction
  const sanitizedText = sanitizeHTML(html); // sanitize the string to remove all HTML tags
  return sanitizedText.trim() || ""; // trim the string to remove leading and trailing whitespaces
};
