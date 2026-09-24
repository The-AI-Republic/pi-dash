/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values
        ? Object.entries(values).reduce((message, [name, value]) => message.replace(`{${name}}`, value), key)
        : key,
  }),
}));

vi.mock("@pi-dash/ui", () => ({
  AlertModalCore: ({
    isOpen,
    title,
    content,
    handleSubmit,
    primaryButtonText,
  }: {
    isOpen: boolean;
    title: string;
    content: React.ReactNode;
    handleSubmit: () => void;
    primaryButtonText: { default: string };
  }) =>
    isOpen ? (
      <div role="dialog" aria-label={title}>
        {content}
        <button type="button" onClick={handleSubmit}>
          {primaryButtonText.default}
        </button>
      </div>
    ) : null,
}));

import { ProjectIdentifierChangeAlert } from "@/components/project/project-identifier-change-alert";

describe("ProjectIdentifierChangeAlert", () => {
  it("warns about disconnected runners and queued runs before confirming a project ID change", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();

    render(
      <ProjectIdentifierChangeAlert
        isOpen
        isSubmitting={false}
        oldIdentifier="AIREPUBLIC"
        newIdentifier="OPENHUB"
        onClose={vi.fn()}
        onConfirm={onConfirm}
      />
    );

    const alert = screen.getByRole("dialog", { name: "Change project ID?" });
    expect(alert).toHaveTextContent(
      "Changing the project ID from AIREPUBLIC to OPENHUB can disconnect local runners configured with the current ID."
    );
    expect(alert).toHaveTextContent(
      "update each affected runner's project_slug to OPENHUB and restart the Pi Dash runner daemon"
    );
    expect(alert).toHaveTextContent("Agent runs may remain queued until the runners reconnect.");

    await user.click(screen.getByRole("button", { name: "Change project ID" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });
});
