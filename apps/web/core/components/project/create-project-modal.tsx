/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
// hooks
import useKeypress from "@/hooks/use-keypress";
// pi dash web components
import { CreateProjectForm } from "@/pi-dash-web/components/projects/create/root";
// pi dash web types
import type { TProject } from "@/pi-dash-web/types/projects";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  setToFavorite?: boolean;
  workspaceSlug: string;
  data?: Partial<TProject>;
  templateId?: string;
};

export function CreateProjectModal(props: Props) {
  const { isOpen, onClose, setToFavorite = false, workspaceSlug, data, templateId } = props;

  useKeypress("Escape", () => {
    if (isOpen) onClose();
  });

  return (
    <ModalCore isOpen={isOpen} position={EModalPosition.TOP} width={EModalWidth.XXXXL}>
      <CreateProjectForm
        setToFavorite={setToFavorite}
        workspaceSlug={workspaceSlug}
        onClose={onClose}
        handleNextStep={() => onClose()}
        data={data}
        templateId={templateId}
      />
    </ModalCore>
  );
}
