// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public marketing feature (CLOUD-003/004): the shared frame, the home
// page, the frame-mounted intro pages, and the head metadata every public
// route owns. Routes import from here; nothing outside the feature reaches
// into components/.

export { MarketingFrame } from "./components/MarketingFrame.js";
export type { MarketingFrameProps } from "./components/MarketingFrame.js";
export { MarketingHome } from "./components/MarketingHome.js";
export { MarketingIntro } from "./components/MarketingIntro.js";
export type { MarketingIntroProps } from "./components/MarketingIntro.js";
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
