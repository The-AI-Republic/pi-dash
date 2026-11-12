// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body markup copy (parity/specs/pages.md §6.1, PAGE-044). The
// options menu's copy entry writes the live editor HTML as Markdown: the
// same text a Markdown paste turns back into blocks. The converter is
// deliberately small and total — every element falls back to its text —
// so an unfamiliar node can never block a copy.

/** Inline text conversion: marks wrapped around their children. */
function inlineMarkdown(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? "";
  if (!(node instanceof Element)) return "";
  const children = Array.from(node.childNodes).map(inlineMarkdown).join("");
  switch (node.tagName.toLowerCase()) {
    case "strong":
    case "b":
      return wrap(children, "**");
    case "em":
    case "i":
      return wrap(children, "_");
    case "s":
    case "del":
      return wrap(children, "~~");
    case "code":
      return wrap(children, "`");
    case "u":
      return `<u>${children}</u>`;
    case "a": {
      const href = node.getAttribute("href") ?? "";
      return href === "" ? children : `[${children}](${href})`;
    }
    case "br":
      return "  \n";
    case "span": {
      if (node.getAttribute("data-type") === "emoji") return children;
      const color = node.getAttribute("data-text-color");
      return color === null ? children : `<span data-text-color="${color}">${children}</span>`;
    }
    case "mention-component": {
      const name = node.getAttribute("data-name") ?? node.getAttribute("entity_identifier") ?? "";
      return name === "" ? "" : `@${name}`;
    }
    case "img":
    case "image-component": {
      const src = node.getAttribute("data-stored-src") ?? node.getAttribute("src") ?? "";
      const alt = node.getAttribute("alt") ?? "";
      return src === "" ? "" : `![${alt}](${src})`;
    }
    default:
      return children;
  }
}

function wrap(text: string, mark: string): string {
  return text.trim() === "" ? text : `${mark}${text}${mark}`;
}

/** One block's Markdown form, or "" when it carries no content. */
function blockMarkdown(element: Element): string {
  const tag = element.tagName.toLowerCase();
  if (tag === "issue-embed-component") {
    const label = element.getAttribute("entity_name") ?? element.getAttribute("entity_identifier") ?? "";
    return label === "" ? "" : `[${label}]`;
  }
  if (
    tag === "callout-component" ||
    tag === "aside" ||
    (tag === "div" && element.getAttribute("data-block-type") === "callout-component")
  ) {
    return childrenBlocks(element)
      .map((line) => `> ${line}`)
      .join("\n");
  }
  if (tag === "pre") {
    const code = element.querySelector("code");
    const language = code?.className.match(/language-([\w-]+)/)?.[1] ?? "";
    return `\`\`\`${language}\n${(code ?? element).textContent ?? ""}\n\`\`\``;
  }
  if (tag === "blockquote") {
    return childrenBlocks(element)
      .map((line) => `> ${line}`)
      .join("\n");
  }
  if (tag === "hr" || (tag === "div" && element.getAttribute("data-type") === "horizontalRule")) return "---";
  if (tag === "ul" || tag === "ol") {
    return listMarkdown(element, tag === "ol");
  }
  if (/^h[1-6]$/.test(tag)) {
    return `${"#".repeat(Number(tag.slice(1)))} ${inlineOf(element)}`;
  }
  if (tag === "table") {
    const rows = Array.from(element.querySelectorAll("tr")).map((row) =>
      Array.from(row.querySelectorAll("th, td")).map((cell) => inlineOf(cell).trim())
    );
    if (rows.length === 0) return "";
    const header = rows[0] ?? [];
    const lines = [`| ${header.join(" | ")} |`, `| ${header.map(() => "---").join(" | ")} |`];
    for (const row of rows.slice(1)) lines.push(`| ${row.join(" | ")} |`);
    return lines.join("\n");
  }
  if (tag === "image-component" || tag === "img") return inlineMarkdown(element);
  return inlineOf(element);
}

function inlineOf(element: Element): string {
  return Array.from(element.childNodes).map(inlineMarkdown).join("");
}

function childrenBlocks(element: Element): string[] {
  return Array.from(element.children)
    .map(blockMarkdown)
    .filter((text) => text.trim() !== "");
}

/**
 * One list item's body: its own content first, then each nested list.
 * The parent's inline text must survive beside a nested list, so only
 * list children are split off as separate blocks.
 */
function listItemBody(item: Element): string {
  const inlineNodes = Array.from(item.childNodes).filter(
    (node) => !(node instanceof Element) || !["ul", "ol"].includes(node.tagName.toLowerCase())
  );
  const own = inlineNodes.map(inlineMarkdown).join("").trim();
  const nested = Array.from(item.children)
    .filter((child) => ["ul", "ol"].includes(child.tagName.toLowerCase()))
    .map(blockMarkdown)
    .filter((text) => text.trim() !== "");
  if (own === "" && nested.length === 0) return inlineOf(item).trim();
  return [own, ...nested]
    .filter((text) => text !== "")
    .join("\n")
    .trim();
}

function listMarkdown(list: Element, ordered: boolean): string {
  const items = Array.from(list.children).filter((child) => child.tagName.toLowerCase() === "li");
  let index = 0;
  return items
    .map((item) => {
      index += 1;
      const marker = ordered ? `${index}.` : item.getAttribute("data-checked") === "true" ? "- [x]" : "- [ ]";
      const bullet = ordered ? marker : item.hasAttribute("data-checked") ? marker : "-";
      const body = listItemBody(item);
      if (body === "") return "";
      const [first, ...rest] = body.split("\n");
      return [`${bullet} ${first}`, ...rest.map((line) => `  ${line}`)].join("\n");
    })
    .filter((text) => text !== "")
    .join("\n");
}

/** Every top-level block as Markdown, separated by one blank line. */
export function pageBodyMarkdown(html: string): string {
  if (html.trim() === "") return "";
  if (typeof globalThis.DOMParser === "undefined") return "";
  const document = new DOMParser().parseFromString(html, "text/html");
  const blocks = Array.from(document.body.children)
    .map(blockMarkdown)
    .filter((text) => text.trim() !== "");
  if (blocks.length === 0) {
    const fallback = (document.body.textContent ?? "").trim();
    return fallback === "" ? "" : `${fallback}\n`;
  }
  return `${blocks.join("\n\n")}\n`;
}
