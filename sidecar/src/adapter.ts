import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, type BrowserContext, type Page } from "playwright-core";
import { FetchTrackPayload, SearchTracksPayload, type Request } from "./protocol.js";

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
      return this.fetchTrack(payload.url, request.timeoutMs);
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

  private async fetchTrack(input: string, timeoutMs: number) {
    const url = validateSourceTrackUrl(input);
    const page = await this.browserPage();
    page.setDefaultTimeout(timeoutMs);
    const response = await page.goto(url.href, { waitUntil: "domcontentloaded", timeout: timeoutMs });
    if (!response || ![200, 206].includes(response.status())) {
      throw new AdapterError("SOURCE_HTTP_ERROR", `1001Tracklists returned HTTP ${response?.status() ?? "unknown"}.`, true);
    }
    await this.assertUsable(page);
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

  private async assertUsable(page: Page): Promise<void> {
    const title = (await page.title()).toLowerCase();
    const body = (await page.locator("body").innerText()).slice(0, 2500).toLowerCase();
    const challengeMarkers = ["just a moment", "captcha", "access denied", "please wait, you will be forwarded", "turnstile"];
    if (challengeMarkers.some(marker => title.includes(marker) || body.includes(marker)) || await page.locator("#turnstile-container").count() > 0) {
      throw new AdapterError("BROWSER_CHALLENGE", "1001Tracklists needs attention in the dedicated Chrome profile.");
    }
    const url = new URL(page.url());
    if (url.origin !== ORIGIN) throw new AdapterError("UNEXPECTED_NAVIGATION", `The source redirected to ${url.origin}.`);
  }
}
