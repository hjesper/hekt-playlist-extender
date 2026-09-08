import { describe, expect, it } from "vitest";
import { validateSourceTrackUrl } from "./adapter.js";
import { FetchTrackPayload, PROTOCOL_VERSION, Request, SearchTracksPayload, errorResponse } from "./protocol.js";

describe("sidecar protocol", () => {
  it("accepts a bounded typed request", () => expect(Request.parse({ version: 1, requestId: "abc", operation: "searchTracks", payload: {}, timeoutMs: 5000 }).requestId).toBe("abc"));
  it("rejects unknown operations", () => expect(() => Request.parse({ version: 1, requestId: "abc", operation: "browseAnywhere", payload: {} })).toThrow());
  it("keeps errors correlated", () => expect(errorResponse("req-7", "PAUSED", "Challenge")).toMatchObject({ version: PROTOCOL_VERSION, requestId: "req-7", ok: false }));
  it("bounds search result counts", () => {
    expect(SearchTracksPayload.parse({artist:"MPH",title:"Raw",limit:25}).limit).toBe(25);
    expect(SearchTracksPayload.parse({artist:"MPH",title:"Raw",version:null}).version).toBeUndefined();
    expect(() => SearchTracksPayload.parse({artist:"MPH",title:"Raw",limit:26})).toThrow();
  });
  it("accepts only a real 1001Tracklists track URL", () => {
    const value = FetchTrackPayload.parse({ url: "https://www.1001tracklists.com/track/abc/example/index.html" });
    expect(validateSourceTrackUrl(value.url).pathname).toBe("/track/abc/example/index.html");
    expect(() => validateSourceTrackUrl("http://www.1001tracklists.com/track/abc/example/index.html")).toThrow(/Only HTTPS/);
    expect(() => validateSourceTrackUrl("https://evil.example/track/abc/example/index.html")).toThrow(/Only HTTPS/);
    expect(() => validateSourceTrackUrl("https://www.1001tracklists.com/search/result.php")).toThrow(/Only HTTPS/);
  });
});
