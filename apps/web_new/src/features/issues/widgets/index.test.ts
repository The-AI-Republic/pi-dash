// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The feature barrel is the only import surface the routes use; every
// widget export must survive it (ISS-179–186).
import { describe, expect, it } from "vitest";

import * as widgets from "./index.js";

describe("widgets barrel", () => {
  it("exposes the widgets, dialogs and helpers the detail route mounts", () => {
    for (const name of [
      "IssueWidgets",
      "LinksWidget",
      "RelationsWidget",
      "RelationRow",
      "AddRelationDialog",
      "LinkDialog",
      "WidgetSection",
      "WidgetAddButton",
      "WidgetActionsProvider",
      "RelationActionMenu",
      "LinkActionButton",
      "useWidgetActions",
      "RELATION_KINDS",
      "RELATION_KIND_SPECS",
      "RELATION_EMPTY_SELECTION",
      "LINK_URL_REQUIRED",
      "normalizeLinkUrl",
      "copyWorkItemUrl",
      "relationRows",
      "relationCount",
      "visibleRelationGroups",
      "isRelationKind",
      "INVERSE_RELATION",
      "faviconUrl",
      "linkTitle",
      "attribution",
      "addedAgo",
      "loadWidgetOpen",
      "storeWidgetOpen",
      "widgetOpenKey",
    ]) {
      expect(widgets, name).toHaveProperty(name);
    }
  });
});
