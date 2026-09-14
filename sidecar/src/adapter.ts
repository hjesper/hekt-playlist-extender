import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, type BrowserContext, type Page } from "playwright-core";
import { FetchAppearancesPayload, FetchTrackPayload, FetchTracklistPayload, SearchTracksPayload, type Request } from "./protocol.js";

const ORIGIN = "https://www.1001tracklists.com";

export class AdapterError extends Error {
  constructor(public code: string, message: string, public retryable = false) {
    super(message);
  }
}

export type TrackSearchResult = {
  providerId: string;
  url: string;
  displayText: string;
};

export function validateSourceTrackUrl(input: string): URL {
  let url: URL;
  try {
    url = new URL(input);
  } catch {
    throw new AdapterError("INVALID_SOURCE_URL", "The track URL is not valid.");
  }
  if (url.protocol !== "https:" || url.hostname !== "www.1001tracklists.com" || !/^\/track\/[^/]+\/.+/.test(url.pathname)) {
    throw new AdapterError("INVALID_SOURCE_URL", "Only HTTPS 1001Tracklists track URLs are allowed.");
  }
  url.hash = "";
  return url;
}

export function validateSourceTracklistUrl(input: string): URL {
  let url: URL;
  try {
    url = new URL(input);
  } catch {
    throw new AdapterError("INVALID_SOURCE_URL", "The tracklist URL is not valid.");
  }
  if (url.protocol !== "https:" || url.hostname !== "www.1001tracklists.com" || !/^\/tracklist\/[^/]+\/.+/.test(url.pathname)) {
    throw new AdapterError("INVALID_SOURCE_URL", "Only HTTPS 1001Tracklists tracklist URLs are allowed.");
  }
  url.hash = "";
  return url;
}

export class SourceAdapter {
  private context?: BrowserContext;
  private page?: Page;

  async execute(request: Request): Promise<unknown> {
    if (request.operation === "searchTracks") {
      const payload = SearchTracksPayload.parse(request.payload);
      return this.searchTracks(payload, request.timeoutMs);
    }
    if (request.operation === "fetchTrack") {
      const payload = FetchTrackPayload.parse(request.payload);
      return this.fetchTrack(payload, request.timeoutMs);
    }
    if (request.operation === "fetchAppearances") {
      const payload = FetchAppearancesPayload.parse(request.payload);
      return this.fetchAppearances(payload, request.timeoutMs);
    }
    if (request.operation === "fetchTracklist") {
      const payload = FetchTracklistPayload.parse(request.payload);
      return this.fetchTracklist(payload, request.timeoutMs);
    }
    throw new AdapterError("OPERATION_NOT_IMPLEMENTED", `${request.operation} is not enabled in the Phase 0 spike.`);
  }

  async close(): Promise<void> {
    await this.context?.close();
  }

  private async browserPage(): Promise<Page> {
    if (this.page && !this.page.isClosed()) return this.page;
    const profile = process.env.HEKT_BROWSER_PROFILE || join(tmpdir(), "hekt-playlist-extender-browser");
    this.context = await chromium.launchPersistentContext(profile, {
      channel: "chrome",
      headless: process.env.HEKT_BROWSER_HEADLESS !== "0",
      viewport: { width: 1280, height: 900 },
    });
    this.page = this.context.pages()[0] ?? await this.context.newPage();
    return this.page;
  }

  private async searchTracks(payload: {artist:string;title:string;version?:string;limit:number}, timeoutMs: number) {
    const page = await this.browserPage();
    page.setDefaultTimeout(timeoutMs);
    if (!page.url().startsWith(ORIGIN)) {
      const response = await page.goto(ORIGIN, { waitUntil: "domcontentloaded", timeout: timeoutMs });
      if (!response?.ok()) throw new AdapterError("SOURCE_HTTP_ERROR", `1001Tracklists returned HTTP ${response?.status() ?? "unknown"}.`, true);
    }
    await this.assertUsable(page);
    const query = [payload.artist, payload.title, payload.version].filter(Boolean).join(" ");
    await page.locator("#sBoxSel").selectOption("2");
    await page.locator("#sBoxInput").fill(query);
    await Promise.all([
      page.waitForURL(`${ORIGIN}/search/result.php*`, { timeout: timeoutMs }),
      page.locator("#sBoxInput").press("Enter"),
    ]);
    await page.waitForLoadState("domcontentloaded");
    await this.assertUsable(page);
    const tracks: TrackSearchResult[] = [];
    const seen = new Set<string>();
    for (const anchor of await page.locator('a[href*="/track/"]').all()) {
      const href = await anchor.getAttribute("href");
      const displayText = (await anchor.innerText()).trim().replace(/\s+/g, " ");
      if (!href || !displayText) continue;
      const url = new URL(href, ORIGIN);
      const match = url.pathname.match(/^\/track\/([^/]+)\//);
      if (!match || seen.has(url.href)) continue;
      seen.add(url.href);
      tracks.push({ providerId: match[1], url: url.href, displayText });
      if (tracks.length >= payload.limit) break;
    }
    return { query, resultUrl: page.url(), tracks };
  }

  private async fetchTrack(
    payload: {url:string;interactive:boolean;challengeTimeoutMs:number},
    timeoutMs: number,
  ) {
    const url = validateSourceTrackUrl(payload.url);
    const page = await this.browserPage();
    page.setDefaultTimeout(timeoutMs);
    const response = await page.goto(url.href, { waitUntil: "domcontentloaded", timeout: timeoutMs });
    if (!response || ![200, 206].includes(response.status())) {
      throw new AdapterError("SOURCE_HTTP_ERROR", `1001Tracklists returned HTTP ${response?.status() ?? "unknown"}.`, true);
    }
    if (await this.hasChallenge(page)) {
      if (!payload.interactive) {
        throw new AdapterError("BROWSER_CHALLENGE", "1001Tracklists needs attention in the dedicated Chrome profile.");
      }
      await this.waitForChallenge(page, payload.challengeTimeoutMs);
      await page.waitForLoadState("domcontentloaded", { timeout: Math.min(timeoutMs, 30_000) });
    }
    await this.assertUsable(page);
    validateSourceTrackUrl(page.url());
    const title = ((await page.locator("h1").first().textContent().catch(() => null)) || await page.title()).trim();
    const appearances: Array<{url:string;label:string}> = [];
    const seen = new Set<string>();
    for (const anchor of await page.locator('a[href*="/tracklist/"]').all()) {
      const href = await anchor.getAttribute("href");
      const label = (await anchor.innerText()).trim().replace(/\s+/g, " ");
      if (!href || !label) continue;
      const url = new URL(href, ORIGIN).href;
      if (seen.has(url)) continue;
      seen.add(url);
      appearances.push({ url, label });
    }
    return { url: page.url(), title, appearances };
  }

  private async fetchAppearances(
    payload: {url:string;interactive:boolean;challengeTimeoutMs:number;limit:number},
    timeoutMs: number,
  ) {
    const requestedUrl = validateSourceTrackUrl(payload.url);
    const firstPage = await this.fetchTrack(payload, timeoutMs) as {
      url: string;
      title: string;
      appearances: Array<{url:string;label:string}>;
    };
    const appearances = [...firstPage.appearances];
    const seenAppearances = new Set(appearances.map(item => item.url));
    const visitedPages = new Set([new URL(firstPage.url).href]);
    const page = await this.browserPage();
    while (appearances.length < payload.limit && visitedPages.size < 10) {
      const nextHref = await page.locator('a[rel="next"], .pagination a, a.page-link').evaluateAll((anchors) => {
        const next = anchors.find((anchor) => {
          const label = (anchor.textContent ?? "").trim().toLowerCase();
          return anchor.getAttribute("rel") === "next" || label === "next" || label === ">" || label === "›";
        });
        return next?.getAttribute("href") ?? null;
      }).catch(() => null);
      if (!nextHref) break;
      const nextUrl = new URL(nextHref, ORIGIN);
      if (nextUrl.origin !== ORIGIN || nextUrl.pathname !== requestedUrl.pathname || visitedPages.has(nextUrl.href)) break;
      visitedPages.add(nextUrl.href);
      const response = await page.goto(nextUrl.href, { waitUntil: "domcontentloaded", timeout: timeoutMs });
      if (!response || ![200, 206].includes(response.status())) {
        throw new AdapterError("SOURCE_HTTP_ERROR", `1001Tracklists returned HTTP ${response?.status() ?? "unknown"}.`, true);
      }
      if (await this.hasChallenge(page)) {
        if (!payload.interactive) throw new AdapterError("BROWSER_CHALLENGE", "1001Tracklists needs attention in the dedicated Chrome profile.");
        await this.waitForChallenge(page, payload.challengeTimeoutMs);
      }
      await this.assertUsable(page);
      for (const anchor of await page.locator('a[href*="/tracklist/"]').all()) {
        const href = await anchor.getAttribute("href");
        const label = (await anchor.innerText()).trim().replace(/\s+/g, " ");
        if (!href || !label) continue;
        const url = new URL(href, ORIGIN).href;
        if (seenAppearances.has(url)) continue;
        seenAppearances.add(url);
        appearances.push({ url, label });
        if (appearances.length >= payload.limit) break;
      }
    }
    return {
      url: requestedUrl.href,
      title: firstPage.title,
      appearances: appearances.slice(0, payload.limit),
      pagesFetched: visitedPages.size,
    };
  }

  private async fetchTracklist(
    payload: {url:string;interactive:boolean;challengeTimeoutMs:number},
    timeoutMs: number,
  ) {
    const url = validateSourceTracklistUrl(payload.url);
    const page = await this.browserPage();
    page.setDefaultTimeout(timeoutMs);
    const response = await page.goto(url.href, { waitUntil: "domcontentloaded", timeout: timeoutMs });
    if (!response || ![200, 206].includes(response.status())) throw new AdapterError("SOURCE_HTTP_ERROR", `1001Tracklists returned HTTP ${response?.status() ?? "unknown"}.`, true);
    if (await this.hasChallenge(page)) {
      if (!payload.interactive) throw new AdapterError("BROWSER_CHALLENGE", "1001Tracklists needs attention in the dedicated Chrome profile.");
      await this.waitForChallenge(page, payload.challengeTimeoutMs);
      await page.waitForLoadState("domcontentloaded", { timeout: Math.min(timeoutMs, 30_000) });
    }
    await this.assertUsable(page);
    validateSourceTracklistUrl(page.url());
    const title = ((await page.locator("h1").first().textContent().catch(() => null)) || await page.title()).trim();
    const entries: Array<{position:number;displayText:string;trackUrl?:string;providerId?:string;cueSeconds?:number}> = [];
    const identifiedRows = page.locator('[data-trackid]');
    const idRows = page.locator('[id^="tlptr"]');
    const rows = await identifiedRows.count() ? identifiedRows : await idRows.count() ? idRows : page.locator('.tlpTog');
    const seenRows = new Set<string>();
    for (let index = 0; index < await rows.count(); index++) {
      const row = rows.nth(index);
      const displayText = (await row.innerText().catch(() => "")).trim().replace(/\s+/g, " ");
      const rowIdentity = (await row.getAttribute("data-trackid").catch(() => null)) ?? (await row.getAttribute("id").catch(() => null)) ?? `${index}:${displayText}`;
      if (!displayText || seenRows.has(rowIdentity)) continue;
      seenRows.add(rowIdentity);
      const href = await row.locator('a[href*="/track/"]').first().getAttribute("href").catch(() => null);
      const trackUrl = href ? new URL(href, ORIGIN).href : undefined;
      const providerId = trackUrl?.match(/\/track\/([^/]+)\//)?.[1] ?? (await row.getAttribute("data-trackid").catch(() => null)) ?? undefined;
      const cue = displayText.match(/(?:^|\s)(\d{1,2}):(\d{2})(?=\s|$)/);
      entries.push({ position: entries.length + 1, displayText, trackUrl, providerId, cueSeconds: cue ? Number(cue[1]) * 60 + Number(cue[2]) : undefined });
      if (entries.length >= 500) break;
    }
    if (!entries.length) throw new AdapterError("SOURCE_MARKUP_CHANGED", "No tracklist entries were recognized; the source markup may have changed.");
    return { url: page.url(), providerId: url.pathname.split("/")[2], title, entries };
  }

  private async waitForChallenge(page: Page, timeoutMs: number): Promise<void> {
    await page.bringToFront();
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (page.isClosed()) {
        throw new AdapterError("BROWSER_SESSION_CLOSED", "The dedicated Chrome window was closed before source access was verified.", true);
      }
      if (!await this.hasChallenge(page)) return;
      await page.waitForTimeout(1_000);
    }
    throw new AdapterError("BROWSER_CHALLENGE_TIMEOUT", "Source access is still waiting for attention in the dedicated Chrome window.", true);
  }

  private async hasChallenge(page: Page): Promise<boolean> {
    const title = (await page.title().catch(() => "")).toLowerCase();
    const body = (await page.locator("body").innerText().catch(() => "")).slice(0, 2500).toLowerCase();
    const challengeMarkers = ["just a moment", "captcha", "access denied", "please wait, you will be forwarded", "turnstile"];
    return challengeMarkers.some(marker => title.includes(marker) || body.includes(marker))
      || await page.locator("#turnstile-container").count().catch(() => 0) > 0;
  }

  private async assertUsable(page: Page): Promise<void> {
    if (await this.hasChallenge(page)) {
      throw new AdapterError("BROWSER_CHALLENGE", "1001Tracklists needs attention in the dedicated Chrome profile.");
    }
    const url = new URL(page.url());
    if (url.origin !== ORIGIN) throw new AdapterError("UNEXPECTED_NAVIGATION", `The source redirected to ${url.origin}.`);
  }
}
