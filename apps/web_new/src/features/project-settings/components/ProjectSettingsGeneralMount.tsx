// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General tab body mount (PSET-001/PSET-005). The full form — artwork,
// execution defaults, danger zone — is the General slice's screen; until
// it lands this body names the project and offers its identity fields so
// the shell's landing tab resolves and renders content. Edits are local
// and discarded on navigation, matching the area's no-confirm rule.

import { Input } from "@pidash/kit";
import * as React from "react";

import { useProjectSettings } from "./ProjectSettingsLayout.js";

export function ProjectSettingsGeneralMount(): React.ReactElement {
  const { project } = useProjectSettings();
  const [name, setName] = React.useState(project?.name ?? "");
  const [identifier, setIdentifier] = React.useState(project?.identifier ?? "");
  return (
    <section
      aria-label="General settings"
      data-testid="project-settings-general"
      data-slice="NEWFRONT-572"
      className="flex flex-col gap-(--space-4)"
    >
      <Input label="Project name" value={name} onChange={(event) => setName(event.currentTarget.value)} />
      <Input label="Short code" value={identifier} onChange={(event) => setIdentifier(event.currentTarget.value)} />
    </section>
  );
}
