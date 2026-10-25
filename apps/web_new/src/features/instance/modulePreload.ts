// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop warm-start preload (NEWFRONT-906). The issues screen body sits
// behind a lazy route boundary so the initial-JS gate only measures the
// shell and first route. On a warm desktop relaunch the browser cannot
// discover that chunk until the route module has loaded and the router has
// booted, so the fetch lands on the critical path. The desktop build injects
// a modulepreload link into index.html; the body stays lazy (and the size
// gate keeps ignoring it), but its download starts with the document.

/** The modulepreload tag for one emitted asset file. */
export function issuesScreenPreloadTag(fileName: string): string {
  return `<link rel="modulepreload" crossorigin href="/${fileName}">`;
}

/**
 * Add the issues-screen preload to a built index.html. Idempotent, so a
 * second build pass over the same document does not duplicate the tag.
 */
export function withIssuesScreenPreload(html: string, fileName: string): string {
  const tag = issuesScreenPreloadTag(fileName);
  if (html.includes(tag)) return html;
  return html.replace("</head>", `${tag}\n</head>`);
}
