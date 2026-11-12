// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy entry to the rich filter builder. It lives beside the modules it
// loads rather than in the issues barrel, so an eager route can prime the
// dialog chunk without pulling every screen the barrel exports into the
// initial bundle (size gate; NEWFRONT-619).

export type RichFilterBuilderModule = {
  useRichFilterBuilder: (typeof import("./RichFilterBuilder.js"))["useRichFilterBuilder"];
  serializeRichExpression: (typeof import("./RichFilterBuilder.js"))["serializeRichExpression"];
  parseRichExpression: (typeof import("./expression.js"))["parseRichExpression"];
  FieldConfigRegistry: (typeof import("./temporary.js"))["FieldConfigRegistry"];
  buildViewCreate: (typeof import("./views.js"))["buildViewCreate"];
  buildViewPatch: (typeof import("./views.js"))["buildViewPatch"];
  viewDisplaySnapshot: (typeof import("./views.js"))["viewDisplaySnapshot"];
  savedViewKeys: (typeof import("./views.js"))["savedViewKeys"];
};

let richFilterBuilderRequest: Promise<RichFilterBuilderModule> | null = null;

/** Start (or reuse) the builder-module request for lazy dialog hosts. */
export function loadRichFilterBuilder(): Promise<RichFilterBuilderModule> {
  richFilterBuilderRequest ??= Promise.all([
    import("./RichFilterBuilder.js"),
    import("./expression.js"),
    import("./views.js"),
    import("./temporary.js"),
  ]).then(([builder, expression, views, temporary]) => ({
    useRichFilterBuilder: builder.useRichFilterBuilder,
    serializeRichExpression: builder.serializeRichExpression,
    parseRichExpression: expression.parseRichExpression,
    FieldConfigRegistry: temporary.FieldConfigRegistry,
    buildViewCreate: views.buildViewCreate,
    buildViewPatch: views.buildViewPatch,
    viewDisplaySnapshot: views.viewDisplaySnapshot,
    savedViewKeys: views.savedViewKeys,
  }));
  return richFilterBuilderRequest;
}

/** Kick the request while the host list's data is still loading. */
export function preloadRichFilterBuilder(): void {
  void loadRichFilterBuilder().catch(() => {
    // The host's filter section surfaces the failure when it renders.
  });
}
