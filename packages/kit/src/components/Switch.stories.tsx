// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { Switch } from "./Switch";

export const States = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
    <Switch defaultChecked={false} aria-label="Off" />
    <Switch defaultChecked aria-label="On" />
    <Switch defaultChecked disabled aria-label="Disabled on" />
  </div>
);

export const ControlledStates = (): React.ReactElement => {
  const [on, setOn] = React.useState(true);
  const [off, setOff] = React.useState(false);
  return (
    <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
      <Switch checked={on} label="Notifications" onCheckedChange={setOn} />
      <Switch checked={off} label="Telemetry" onCheckedChange={setOff} />
    </div>
  );
};

export const Disabled = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
    <Switch checked label="Locked on" onCheckedChange={() => {}} disabled />
    <Switch checked={false} label="Locked off" onCheckedChange={() => {}} disabled />
  </div>
);
