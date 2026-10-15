// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Switch } from "./Switch";

export const States = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
    <Switch defaultChecked={false} aria-label="Off" />
    <Switch defaultChecked aria-label="On" />
    <Switch defaultChecked disabled aria-label="Disabled on" />
  </div>
);
