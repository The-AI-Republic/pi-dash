// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ProgressRing } from "./ProgressRing";

export const Rings = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
    <ProgressRing percent={0} />
    <ProgressRing percent={35} />
    <ProgressRing percent={72} />
    <ProgressRing percent={100} />
    <ProgressRing percent={44} size="small" />
  </div>
);
