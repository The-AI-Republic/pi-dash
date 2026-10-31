// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  nextTourStep,
  previousTourStep,
  shouldShowProductTour,
  tourContentStop,
  TOUR_CONTENT_STOPS,
  TOUR_VIDEO_EMBED_URL,
  tourWelcomeGreeting,
} from "./tour.js";

describe("tour", () => {
  it("shows the tour only for a loaded profile whose flag is still false", () => {
    expect(shouldShowProductTour(null)).toBe(false);
    expect(shouldShowProductTour(undefined)).toBe(false);
    expect(shouldShowProductTour({ is_tour_completed: true })).toBe(false);
    expect(shouldShowProductTour({ is_tour_completed: false })).toBe(true);
  });

  it("greets named users personally and unnamed users plainly", () => {
    expect(tourWelcomeGreeting("Ada")).toBe("Welcome to Pi Dash, Ada");
    expect(tourWelcomeGreeting("  Ada  ")).toBe("Welcome to Pi Dash, Ada");
    expect(tourWelcomeGreeting("")).toBe("Welcome to Pi Dash");
    expect(tourWelcomeGreeting("   ")).toBe("Welcome to Pi Dash");
  });

  it("walks welcome through the five stops in order", () => {
    expect(nextTourStep("welcome")).toBe("work-items");
    expect(nextTourStep("work-items")).toBe("runners");
    expect(nextTourStep("runners")).toBe("chat");
    expect(nextTourStep("chat")).toBe("workpads");
    expect(nextTourStep("workpads")).toBe("pages");
    expect(nextTourStep("pages")).toBeNull();
  });

  it("steps nowhere from an unknown key", () => {
    const unknown = "archived" as unknown as Parameters<typeof nextTourStep>[0];
    expect(nextTourStep(unknown)).toBeNull();
    expect(previousTourStep(unknown)).toBeNull();
  });

  it("steps back except from welcome and the first stop", () => {
    expect(previousTourStep("welcome")).toBeNull();
    expect(previousTourStep("work-items")).toBeNull();
    expect(previousTourStep("runners")).toBe("work-items");
    expect(previousTourStep("chat")).toBe("runners");
    expect(previousTourStep("workpads")).toBe("chat");
    expect(previousTourStep("pages")).toBe("workpads");
  });

  it("resolves each content stop and none for welcome", () => {
    expect(TOUR_CONTENT_STOPS).toHaveLength(5);
    expect(TOUR_CONTENT_STOPS.map((stop) => stop.key)).toEqual(["work-items", "runners", "chat", "workpads", "pages"]);
    expect(tourContentStop("welcome")).toBeNull();
    for (const stop of TOUR_CONTENT_STOPS) {
      expect(tourContentStop(stop.key)).toBe(stop);
      expect(stop.navLabel.trim()).not.toBe("");
      expect(stop.title.trim()).not.toBe("");
      expect(stop.body.trim()).not.toBe("");
    }
  });

  it("embeds the product concept video over https", () => {
    expect(TOUR_VIDEO_EMBED_URL.startsWith("https://")).toBe(true);
  });
});
