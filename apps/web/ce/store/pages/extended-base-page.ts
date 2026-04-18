/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TPage, TPageExtended } from "@apple-pi-dash/types";
import type { RootStore } from "@/apple-pi-dash-web/store/root.store";
import type { TBasePageServices } from "@/store/pages/base-page";

export type TExtendedPageInstance = TPageExtended & {
  asJSONExtended: TPageExtended;
};

export class ExtendedBasePage implements TExtendedPageInstance {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  constructor(store: RootStore, page: TPage, services: TBasePageServices) {}

  get asJSONExtended(): TExtendedPageInstance["asJSONExtended"] {
    return {};
  }
}
