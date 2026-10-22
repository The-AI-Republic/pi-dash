// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver marketing reads (NEWFRONT-645,
// CLOUD-003/004; NEWFRONT-648, CLOUD-006–009). A stub Page stands in for
// Playwright; every assertion is about the selector or time the method
// reads back, not about a browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: () => Promise<void>;
  waitFor?: () => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string | null>;
  evaluate?: (fn: (root: never) => unknown) => Promise<unknown>;
  focus?: () => Promise<void>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  getByText?: (text: string | RegExp) => StubLocator;
  locator?: (selector: string) => StubLocator;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    evaluate: async () => null,
    focus: async () => undefined,
  };
  node.first = () => node;
  node.last = () => node;
  node.nth = () => node;
  node.getByRole = () => node;
  node.getByTestId = () => node;
  node.getByText = () => node;
  node.locator = () => node;
  Object.assign(node, overrides);
  return node as unknown as Locator;
}

function pageOf(locatorFor: (selector: string) => Locator): Page {
  return {
    locator: (selector: string) => locatorFor(selector),
    getByRole: () => locator(),
    waitForLoadState: async () => undefined,
  } as unknown as Page;
}

describe("cloud frame reads", () => {
  it("reads header links through the first header, with labels and external targets", async () => {
    const anchors = [
      locator({ evaluate: async () => ({ label: "Downloads", href: "/downloads", external: false }) }),
      locator({ evaluate: async () => ({ label: "GitHub", href: "https://github.com/x", external: true }) }),
    ];
    const header = locator({
      locator: (selector: string) => {
        if (selector === "a") return locator({ count: async () => anchors.length, nth: (i) => anchors[i]! });
        return locator();
      },
    });
    const page = pageOf((selector) => (selector === "header" ? header : locator()));
    await expect(new WebNewDriver(page).cloudHeaderLinks()).resolves.toEqual([
      { label: "Downloads", href: "/downloads", external: false, visible: true },
      { label: "GitHub", href: "https://github.com/x", external: true, visible: true },
    ]);
  });

  it("skips hidden links and reports false when a visible link has no ring", async () => {
    const hidden = locator({ isVisible: async () => false, evaluate: async () => false });
    const visible = locator({ isVisible: async () => true, evaluate: async () => true });
    const footer = locator({
      locator: (selector: string) => {
        if (selector === "a") return locator({ count: async () => 2, nth: (i) => (i === 0 ? hidden : visible) });
        return locator();
      },
    });
    const page = pageOf((selector) => (selector === "footer" ? footer : locator()));
    await expect(new WebNewDriver(page).cloudFooterLinksFocusVisible()).resolves.toBe(true);

    const ringless = locator({ evaluate: async () => false });
    const ringed = locator({ evaluate: async () => true });
    const mixed = locator({
      locator: (selector: string) => {
        if (selector === "a") return locator({ count: async () => 2, nth: (i) => (i === 0 ? ringed : ringless) });
        return locator();
      },
    });
    const mixedPage = pageOf((selector) => (selector === "footer" ? mixed : locator()));
    await expect(new WebNewDriver(mixedPage).cloudFooterLinksFocusVisible()).resolves.toBe(false);
  });
});

describe("cloud home reads", () => {
  it("reads the hero action hrefs in DOM order", async () => {
    const links = [
      locator({ getAttribute: async () => "/login" }),
      locator({ getAttribute: async () => "/downloads" }),
      locator({ getAttribute: async () => "/docs" }),
    ];
    const hero = locator({
      getByRole: () => locator({ count: async () => links.length, nth: (i) => links[i]! }),
    });
    const main = locator({
      locator: () =>
        locator({
          first: () => hero,
          locator: () => locator(),
        }),
    });
    const page = pageOf(() => main);
    await expect(new WebNewDriver(page).cloudHomeHeroActionHrefs()).resolves.toEqual(["/login", "/downloads", "/docs"]);
  });

  it("clicks the hero action with the requested href and waits for the next page", async () => {
    const clicked: string[] = [];
    const action = locator({ click: async () => void clicked.push("downloads") });
    const hero = locator({
      locator: (selector: string) => (selector === 'a[href="/downloads"]' ? action : locator()),
    });
    const main = locator({
      locator: (selector: string) => (selector === "section" ? locator({ first: () => hero }) : locator()),
    });
    const page = pageOf(() => main);
    await new WebNewDriver(page).cloudHomeClickHeroAction("/downloads");
    expect(clicked).toEqual(["downloads"]);
  });

  it("counts the strip items, steps, capability cards and reads the repo and closing link", async () => {
    const strip = locator({
      locator: (selector: string) =>
        selector === ":scope > div > div" ? locator({ count: async () => 4 }) : locator(),
    });
    const sections = locator({
      count: async () => 6,
      nth: (index: number) => {
        if (index === 1) return strip;
        if (index === 5) return locator({ getByRole: () => locator({ getAttribute: async () => "/login" }) });
        return locator();
      },
    });
    const main = locator({
      locator: (selector: string) => {
        if (selector === "section") return sections;
        if (selector === "ol > li") return locator({ count: async () => 4 });
        if (selector === "article") return locator({ count: async () => 4 });
        if (selector.includes("github.com")) {
          return locator({
            getAttribute: async (name: string) =>
              name === "href" ? "https://github.com/The-AI-Republic/pi-dash" : "_blank",
          });
        }
        return locator();
      },
    });
    const page = pageOf(() => main);
    await expect(new WebNewDriver(page).cloudHomeSections()).resolves.toEqual({
      stripItems: 4,
      workflowSteps: 4,
      capabilityCards: 4,
      repoHref: "https://github.com/The-AI-Republic/pi-dash",
      repoExternal: true,
      closingCtaHref: "/login",
    });
  });
});

const CLI_MSI = "https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi";
const REPOSITORY = "https://github.com/The-AI-Republic/pi-dash";

/** A locator whose first() is a link carrying href (or missing/absent). */
function firstLink(href: string | null, present = true): Locator {
  const link = locator({ count: async () => (present ? 1 : 0), getAttribute: async () => href });
  return locator({ first: () => link });
}

describe("cloud docs reads", () => {
  function docsPage(blocks: Locator[], links: Record<string, Locator>, footerItems: number): Page {
    const main = locator({
      locator: (selector: string) => {
        if (selector === "article") return locator({ count: async () => 3 });
        if (selector === "ol > li") return locator({ count: async () => 4 });
        if (selector === "pre") return locator({ count: async () => blocks.length, nth: (index) => blocks[index]! });
        if (selector === "section") {
          const closing = locator({
            locator: (inner: string) =>
              inner === ":scope > div > div" ? locator({ count: async () => footerItems }) : locator(),
          });
          return locator({ last: () => closing });
        }
        return links[selector] ?? locator();
      },
    });
    return pageOf(() => main);
  }

  it("reads concepts, steps, code blocks, links and trust notes", async () => {
    const blocks = [
      "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.sh | sh",
      "pidash auth login",
    ].map((text) => locator({ innerText: async () => text }));
    const page = docsPage(
      blocks,
      {
        'a[href$=".msi"]': firstLink(CLI_MSI),
        'a[href="/login"]': firstLink("/login"),
        'a[href*="github.com/The-AI-Republic/pi-dash"]': firstLink(REPOSITORY),
      },
      3
    );
    await expect(new WebNewDriver(page).cloudDocsFacts()).resolves.toEqual({
      concepts: 3,
      steps: 4,
      codeBlocks: 2,
      installRefsReleaseDownload: true,
      msiHref: CLI_MSI,
      signInHref: "/login",
      repoHref: REPOSITORY,
      footerItems: 3,
    });
  });

  it("nulls absent docs links and tolerates missing block text", async () => {
    const blocks = [locator({ innerText: async () => null }), locator({ innerText: async () => "pidash auth login" })];
    const page = docsPage(
      blocks,
      {
        'a[href$=".msi"]': firstLink(null, false),
        'a[href="/login"]': firstLink(null),
        'a[href*="github.com/The-AI-Republic/pi-dash"]': firstLink(null, false),
      },
      0
    );
    await expect(new WebNewDriver(page).cloudDocsFacts()).resolves.toEqual({
      concepts: 3,
      steps: 4,
      codeBlocks: 2,
      installRefsReleaseDownload: false,
      msiHref: null,
      signInHref: null,
      repoHref: null,
      footerItems: 0,
    });
  });

  it("nulls present-but-href-less docs links", async () => {
    const blocks = [locator({ innerText: async () => "nothing pinned" })];
    const page = docsPage(
      blocks,
      {
        'a[href$=".msi"]': firstLink(null),
        'a[href="/login"]': firstLink(null, false),
        'a[href*="github.com/The-AI-Republic/pi-dash"]': firstLink(null),
      },
      0
    );
    const facts = await new WebNewDriver(page).cloudDocsFacts();
    expect(facts.msiHref).toBeNull();
    expect(facts.signInHref).toBeNull();
    expect(facts.repoHref).toBeNull();
    expect(facts.installRefsReleaseDownload).toBe(false);
  });
});

describe("cloud downloads reads", () => {
  function card(os: string | null, arch: string | null, hrefs: (string | null)[]): Locator {
    const links = hrefs.map((href) => locator({ getAttribute: async () => href }));
    return locator({
      locator: (selector: string) => {
        if (selector === "h3") return locator({ first: () => locator({ innerText: async () => os }) });
        if (selector === "p") return locator({ first: () => locator({ innerText: async () => arch }) });
        return locator();
      },
      getByRole: () =>
        locator({ count: async () => links.length, first: () => links[0]!, nth: (index) => links[index]! }),
    });
  }

  function downloadsPage(cards: Locator[], notice: number): Page {
    const main = locator({
      locator: (selector: string) => {
        if (selector === "article") return locator({ count: async () => cards.length, nth: (index) => cards[index]! });
        return locator();
      },
      getByText: () => locator({ count: async () => notice }),
    });
    return pageOf(() => main);
  }

  it("reads each OS card with arch notes and primary/secondary hrefs", async () => {
    const base = "https://pidash-desktop-releases.s3.us-west-2.amazonaws.com/downloads/latest";
    const page = downloadsPage(
      [
        card("macOS", "Universal disk image · Apple silicon and Intel", [`${base}/pidash-macos-universal.dmg`]),
        card("Windows", "x64 installer · MSI available", [
          `${base}/pidash-windows-x64-setup.exe`,
          `${base}/pidash-windows-x64.msi`,
        ]),
        card("Linux", "x64 AppImage · Debian package available", [
          `${base}/pidash-linux-x64.AppImage`,
          `${base}/pidash-linux-x64.deb`,
        ]),
      ],
      1
    );
    await expect(new WebNewDriver(page).cloudDownloadCards()).resolves.toEqual([
      {
        os: "macOS",
        arch: "Universal disk image · Apple silicon and Intel",
        primaryHref: `${base}/pidash-macos-universal.dmg`,
        secondaryHref: null,
      },
      {
        os: "Windows",
        arch: "x64 installer · MSI available",
        primaryHref: `${base}/pidash-windows-x64-setup.exe`,
        secondaryHref: `${base}/pidash-windows-x64.msi`,
      },
      {
        os: "Linux",
        arch: "x64 AppImage · Debian package available",
        primaryHref: `${base}/pidash-linux-x64.AppImage`,
        secondaryHref: `${base}/pidash-linux-x64.deb`,
      },
    ]);
  });

  it("reports empty facts for href-less and link-less cards", async () => {
    const page = downloadsPage([card(null, null, []), card("  Windows\n", "x64", [null])], 0);
    await expect(new WebNewDriver(page).cloudDownloadCards()).resolves.toEqual([
      { os: "", arch: "", primaryHref: "", secondaryHref: null },
      { os: "Windows", arch: "x64", primaryHref: "", secondaryHref: null },
    ]);
  });

  it("reports the signing notice by its pinned wording", async () => {
    await expect(new WebNewDriver(downloadsPage([], 1)).cloudDownloadsSigningNoticeVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(downloadsPage([], 0)).cloudDownloadsSigningNoticeVisible()).resolves.toBe(false);
  });
});

describe("cloud CLI card reads", () => {
  function cliPage(installer: Locator, docs: Locator, web: Locator, hint: number): Page {
    const cli = locator({
      locator: (selector: string) => {
        if (selector === 'a[href$=".msi"]') return installer;
        if (selector === 'a[href="/docs"]') return docs;
        return locator();
      },
      getByText: () => locator({ count: async () => hint }),
    });
    const closing = locator({
      locator: (selector: string) => (selector === 'a[href="/login"]' ? web : locator()),
    });
    const main = locator({
      locator: (selector: string) => {
        if (selector === "#cli-heading") return locator({ locator: () => cli });
        if (selector === "section") return locator({ last: () => closing });
        return locator();
      },
    });
    return pageOf(() => main);
  }

  it("reads the installer, docs link, web fallback and sign-in hint", async () => {
    const page = cliPage(firstLink(CLI_MSI), firstLink("/docs"), firstLink("/login"), 1);
    await expect(new WebNewDriver(page).cloudCliCardFacts()).resolves.toEqual({
      installerHref: CLI_MSI,
      docsHref: "/docs",
      webHref: "/login",
      signinHintVisible: true,
    });
  });

  it("reports empty facts when the CLI card links are absent", async () => {
    const page = cliPage(firstLink(null, false), firstLink(null), firstLink(null, false), 0);
    await expect(new WebNewDriver(page).cloudCliCardFacts()).resolves.toEqual({
      installerHref: "",
      docsHref: null,
      webHref: null,
      signinHintVisible: false,
    });
  });

  it("reports empty destinations for present-but-href-less CLI links", async () => {
    const page = cliPage(firstLink(null), firstLink(null, false), firstLink(null), 1);
    await expect(new WebNewDriver(page).cloudCliCardFacts()).resolves.toEqual({
      installerHref: "",
      docsHref: null,
      webHref: null,
      signinHintVisible: true,
    });
  });
});

describe("cloud pricing reads", () => {
  function tier(
    name: string | null,
    price: string | null,
    href: string | null,
    target: string | null,
    ctaPresent: boolean,
    features: number,
    text: string
  ): Locator {
    const cta = locator({
      count: async () => (ctaPresent ? 1 : 0),
      getAttribute: async (attr: string) => (attr === "href" ? href : attr === "target" ? target : null),
    });
    return locator({
      innerText: async () => text,
      locator: (selector: string) => {
        if (selector === "p") return locator({ first: () => locator({ innerText: async () => name }) });
        if (selector === "a") return locator({ count: async () => (ctaPresent ? 1 : 0), last: () => cta });
        if (selector === "ul > li") return locator({ count: async () => features });
        return locator();
      },
      getByTestId: () => locator({ innerText: async () => price }),
    });
  }

  function tiersPage(tiers: Locator[]): Page {
    const main = locator({
      getByTestId: () =>
        locator({
          count: async () => tiers.length,
          nth: (index) => tiers[index]!,
          first: () => tiers[0] ?? locator({ count: async () => 0 }),
        }),
    });
    return pageOf(() => main);
  }

  it("reads tier names, prices, CTAs and feature counts", async () => {
    const page = tiersPage([
      tier("Free + pay as you go", "$0", "/login", null, true, 5, "Up to 500 created issues"),
      tier(
        "  AI Republic membership\n",
        "  $7.99 ",
        "https://airepublic.com/pricing",
        "_blank",
        true,
        5,
        "Uncapped issue creation"
      ),
    ]);
    await expect(new WebNewDriver(page).cloudPricingTiers()).resolves.toEqual([
      { name: "Free + pay as you go", price: "$0", ctaHref: "/login", ctaExternal: false, features: 5 },
      {
        name: "AI Republic membership",
        price: "$7.99",
        ctaHref: "https://airepublic.com/pricing",
        ctaExternal: true,
        features: 5,
      },
    ]);
  });

  it("reports empty facts for a tier without links or text", async () => {
    const page = tiersPage([tier(null, null, null, null, false, 0, "")]);
    await expect(new WebNewDriver(page).cloudPricingTiers()).resolves.toEqual([
      { name: "", price: "", ctaHref: "", ctaExternal: false, features: 0 },
    ]);
  });

  it("reports an empty destination for a href-less CTA", async () => {
    const page = tiersPage([tier("Free + pay as you go", "$0", null, "_blank", true, 5, "500")]);
    await expect(new WebNewDriver(page).cloudPricingTiers()).resolves.toEqual([
      { name: "Free + pay as you go", price: "$0", ctaHref: "", ctaExternal: true, features: 5 },
    ]);
  });

  it("reads the enterprise contact destination", async () => {
    const main = locator({
      locator: (selector: string) =>
        selector === 'a[href*="airepublic.com/contact"]' ? firstLink("https://airepublic.com/contact") : locator(),
    });
    await expect(new WebNewDriver(pageOf(() => main)).cloudPricingEnterpriseHref()).resolves.toBe(
      "https://airepublic.com/contact"
    );
  });

  it("reports no enterprise destination when the link is absent", async () => {
    const main = locator({
      locator: (selector: string) =>
        selector === 'a[href*="airepublic.com/contact"]' ? firstLink(null, false) : locator(),
    });
    await expect(new WebNewDriver(pageOf(() => main)).cloudPricingEnterpriseHref()).resolves.toBeNull();
  });

  it("reports no enterprise destination when the link has no href", async () => {
    const main = locator({
      locator: (selector: string) => (selector === 'a[href*="airepublic.com/contact"]' ? firstLink(null) : locator()),
    });
    await expect(new WebNewDriver(pageOf(() => main)).cloudPricingEnterpriseHref()).resolves.toBeNull();
  });

  it("counts the FAQ entries in the closing section", async () => {
    const closing = locator({ locator: () => locator({ count: async () => 4 }) });
    const main = locator({
      locator: (selector: string) => (selector === "section" ? locator({ last: () => closing }) : locator()),
    });
    await expect(new WebNewDriver(pageOf(() => main)).cloudPricingFaqCount()).resolves.toBe(4);
  });

  it("spots the free cap mention in the first tier", async () => {
    const page = tiersPage([tier("Free + pay as you go", "$0", "/login", null, true, 5, "Up to 500 created issues")]);
    await expect(new WebNewDriver(page).cloudPricingFreeCapMentioned()).resolves.toBe(true);
  });

  it("reports no cap mention without tiers", async () => {
    await expect(new WebNewDriver(tiersPage([])).cloudPricingFreeCapMentioned()).resolves.toBe(false);
  });

  it("reports no cap mention when the tier text is missing", async () => {
    const free = locator({ count: async () => 1, innerText: async () => null });
    const main = locator({ getByTestId: () => locator({ first: () => free }) });
    await expect(new WebNewDriver(pageOf(() => main)).cloudPricingFreeCapMentioned()).resolves.toBe(false);
  });
});
