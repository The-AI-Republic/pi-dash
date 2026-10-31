// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Type bridge to the lazily loaded plotting library (Stack: heavy
// dependencies are only reached through import()). Source files must
// never statically import the library — not even its types — so
// plots receive the loaded module value and derive the callback
// prop types they need from it. Tooltip and legend content need no
// library types: they render as content elements reading the
// injected payload defensively (see ChartTooltip.tsx).
import type * as React from "react";

/** The plotting library module, as a dynamic import resolves it. */
export type RechartsModule = typeof import("recharts");

type PolarAngleTickProp = NonNullable<React.ComponentProps<RechartsModule["PolarAngleAxis"]>["tick"]>;
type PolarAngleTickFn = Extract<PolarAngleTickProp, (...args: never[]) => unknown>;

/** Render-props the angle axis passes to a tick function. */
export type RadarTickProps = Parameters<PolarAngleTickFn>[0];

type BarShapeProp = NonNullable<React.ComponentProps<RechartsModule["Bar"]>["shape"]>;
type BarShapeFn = Extract<BarShapeProp, (...args: never[]) => unknown>;

/** Render-props a bar segment passes to a shape function. */
export type BarShapeRenderProps = Parameters<BarShapeFn>[0];

type LegendSorterProp = NonNullable<React.ComponentProps<RechartsModule["Legend"]>["itemSorter"]>;
type LegendSorterFn = Extract<LegendSorterProp, (...args: never[]) => unknown>;

/** One legend entry the library passes to an item-sorter function. */
export type LegendSorterEntry = Parameters<LegendSorterFn>[0];

/** Item-sorter callback shape the legend accepts. */
export type LegendSorter = (entry: LegendSorterEntry) => number | string;
