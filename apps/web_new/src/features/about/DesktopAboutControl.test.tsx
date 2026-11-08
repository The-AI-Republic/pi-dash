// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop About dialog (DESK-028): no entry without the capability; the
// button loads the shell's About payload and the dialog prints the version
// and every bundled notice, missing ones included; a failed load reports
// in place and retries.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AboutApi, AboutInfo } from "../../core/platform/index.js";
import {
  ABOUT_CONTROL_LABEL,
  ABOUT_LICENSING_SUMMARY,
  ABOUT_RETRY_LABEL,
  DesktopAboutControl,
  DesktopAboutControlView,
  noticeMissingText,
} from "./DesktopAboutControl.js";

const harness = vi.hoisted(() => ({ about: undefined as AboutApi | undefined }));

vi.mock("../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../core/platform/index.js")>();
  return {
    ...original,
    platform: {
      get about() {
        return harness.about;
      },
    },
  };
});

const INFO: AboutInfo = {
  version: "1.4.0",
  notices: [
    {
      file: "pidash-LICENSE.txt",
      component: "Pi Dash",
      license: "AGPL-3.0-only",
      text: "GNU AFFERO GENERAL PUBLIC LICENSE",
    },
    {
      file: "agent-engine-LICENSE.txt",
      component: "Agent engine",
      license: "Apache-2.0",
      text: "Apache License\nVersion 2.0",
    },
    { file: "agent-engine-NOTICE.txt", component: "Agent engine notice", license: "Apache-2.0", text: null },
  ],
};

function capability(load: AboutApi["load"]): AboutApi & { load: ReturnType<typeof vi.fn> } {
  return { load: vi.fn(load) };
}

describe("DesktopAboutControl", () => {
  it("renders nothing without the About capability (web bundle)", () => {
    harness.about = undefined;
    const { container } = render(<DesktopAboutControl />);
    expect(container).toBeEmptyDOMElement();
  });

  it("reads the platform's About capability", async () => {
    const user = userEvent.setup();
    const about = capability(async () => INFO);
    harness.about = about;
    render(<DesktopAboutControl />);
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));
    expect(about.load).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Version 1.4.0")).toBeInTheDocument();
    harness.about = undefined;
  });
});

describe("DesktopAboutControlView", () => {
  it("does not load anything until opened", () => {
    const about = capability(async () => INFO);
    render(<DesktopAboutControlView about={about} />);
    expect(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL })).toBeInTheDocument();
    expect(about.load).not.toHaveBeenCalled();
  });

  it("shows the version, the licensing summary and every bundled text", async () => {
    const user = userEvent.setup();
    render(<DesktopAboutControlView about={capability(async () => INFO)} />);
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));

    const dialog = await screen.findByRole("dialog", { name: ABOUT_CONTROL_LABEL });
    expect(await within(dialog).findByText("Version 1.4.0")).toBeInTheDocument();
    expect(within(dialog).getByText(ABOUT_LICENSING_SUMMARY)).toBeInTheDocument();
    expect(ABOUT_LICENSING_SUMMARY).toContain("pidash-agent-engine");

    const pidash = within(dialog).getByRole("region", { name: "Pi Dash — AGPL-3.0-only" });
    expect(within(pidash).getByText("GNU AFFERO GENERAL PUBLIC LICENSE")).toBeInTheDocument();
    const engine = within(dialog).getByRole("region", { name: "Agent engine — Apache-2.0" });
    // Licence text keeps its line breaks.
    expect(engine.querySelector("pre")?.textContent).toBe("Apache License\nVersion 2.0");
  });

  it("lists a notice this build did not stage as missing", async () => {
    const user = userEvent.setup();
    render(<DesktopAboutControlView about={capability(async () => INFO)} />);
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));
    const notice = await screen.findByRole("region", { name: "Agent engine notice — Apache-2.0" });
    expect(within(notice).getByText(noticeMissingText("agent-engine-NOTICE.txt"))).toBeInTheDocument();
  });

  it("reports a failed load in place and retries", async () => {
    const user = userEvent.setup();
    let fail = true;
    const about = capability(async () => {
      if (fail) throw new Error("resource dir: missing");
      return INFO;
    });
    render(<DesktopAboutControlView about={about} />);
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The licence texts could not be loaded: resource dir: missing");

    fail = false;
    await user.click(within(alert).getByRole("button", { name: ABOUT_RETRY_LABEL }));
    expect(await screen.findByText("Version 1.4.0")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(about.load).toHaveBeenCalledTimes(2);
  });

  it("reloads on every open", async () => {
    const user = userEvent.setup();
    const about = capability(async () => INFO);
    render(<DesktopAboutControlView about={about} />);
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));
    const dialog = await screen.findByRole("dialog", { name: ABOUT_CONTROL_LABEL });
    await user.click(within(dialog).getByRole("button", { name: /close/i }));
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));
    expect(about.load).toHaveBeenCalledTimes(2);
  });

  it("ignores a load that settles after a newer one started", async () => {
    const user = userEvent.setup();
    const resolvers: Array<(info: AboutInfo) => void> = [];
    const about = capability(
      () =>
        new Promise<AboutInfo>((resolve) => {
          resolvers.push(resolve);
        })
    );
    render(<DesktopAboutControlView about={about} />);
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));
    const dialog = await screen.findByRole("dialog", { name: ABOUT_CONTROL_LABEL });
    await user.click(within(dialog).getByRole("button", { name: /close/i }));
    await user.click(screen.getByRole("button", { name: ABOUT_CONTROL_LABEL }));

    resolvers[1]!({ ...INFO, version: "2.0.0" });
    expect(await screen.findByText("Version 2.0.0")).toBeInTheDocument();
    resolvers[0]!(INFO);
    await Promise.resolve();
    expect(screen.queryByText("Version 1.4.0")).not.toBeInTheDocument();
  });
});
