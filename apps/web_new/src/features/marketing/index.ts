// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public marketing feature (CLOUD-003/004/006–009): the shared frame,
// the home, docs, downloads and pricing pages, and the head metadata
// every public route owns. Routes import from here; nothing outside the
// feature reaches into components/.

export { MarketingDocs } from "./components/MarketingDocs.js";
export { MarketingDownloads } from "./components/MarketingDownloads.js";
export { MarketingFrame } from "./components/MarketingFrame.js";
export type { MarketingFrameProps } from "./components/MarketingFrame.js";
export { MarketingHome } from "./components/MarketingHome.js";
export { MarketingPricing } from "./components/MarketingPricing.js";
export {
  MarketingHead,
  SOCIAL_PREVIEW_ALT,
  SOCIAL_PREVIEW_PATH,
  SOCIAL_SITE_HANDLE,
  canonicalUrl,
  marketingTags,
} from "./components/MarketingHead.js";
export type { MarketingHeadProps } from "./components/MarketingHead.js";
export { MARKETING_META, metaFor, type MarketingPageMeta } from "./content.js";
export {
  DEFAULT_PUBLIC_SITE_ORIGIN,
  bakedPublicSiteOrigin,
  publicSiteOriginFromEnv,
} from "../../core/platform/publicSiteOrigin.js";
