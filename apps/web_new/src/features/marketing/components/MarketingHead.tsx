// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Document head for a public marketing page (CLOUD-005): index/follow,
// canonical address, description, and social preview data. The served
// document already carries the same tags from the prerender step; this
// keeps them correct for a client-side navigation to the page. The shell
// owns only the generic title/robots rule, so the tag table lives here in
// the lazy marketing chunk.
import { useDocumentHead, type HeadTag } from "../../../shared/shell/index.js";
import { metaFor, PUBLIC_SITE_ORIGIN, type MarketingPageMeta } from "../content.js";

/** Social preview image served from the app's public directory. */
export const SOCIAL_PREVIEW_PATH = "/social-card.svg";
export const SOCIAL_PREVIEW_ALT = "Pi Dash — projects, issues and agents in one place";
export const SOCIAL_SITE_HANDLE = "@airepublic";

export interface MarketingHeadProps {
  path: string;
}

export function canonicalUrl(path: string, origin: string = PUBLIC_SITE_ORIGIN): string {
  return `${origin.replace(/\/+$/, "")}${path}`;
}

/** Crawl, canonical and social tags for one address. */
export function marketingTags(path: string, meta: MarketingPageMeta): HeadTag[] {
  const canonical = canonicalUrl(path);
  const metaTag = (attributes: Record<string, string>): HeadTag => ({ tag: "meta", attributes });
  return [
    { tag: "link", attributes: { rel: "canonical", href: canonical } },
    metaTag({ name: "description", content: meta.description }),
    metaTag({ property: "og:type", content: "website" }),
    metaTag({ property: "og:title", content: meta.title }),
    metaTag({ property: "og:description", content: meta.description }),
    metaTag({ property: "og:url", content: canonical }),
    metaTag({ property: "og:image", content: SOCIAL_PREVIEW_PATH }),
    metaTag({ property: "og:image:width", content: "1200" }),
    metaTag({ property: "og:image:height", content: "630" }),
    metaTag({ property: "og:image:alt", content: SOCIAL_PREVIEW_ALT }),
    metaTag({ name: "twitter:card", content: "summary_large_image" }),
    metaTag({ name: "twitter:site", content: SOCIAL_SITE_HANDLE }),
  ];
}

export function MarketingHead({ path }: MarketingHeadProps): null {
  const meta: MarketingPageMeta = metaFor(path);
  useDocumentHead({
    title: meta.title,
    robots: "index, follow",
    tags: marketingTags(path, meta),
  });
  return null;
}
