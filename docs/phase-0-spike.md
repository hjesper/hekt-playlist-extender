# Phase 0 spike findings

Date: 2026-09-08

This is a feasibility checkpoint, not a completed Phase 0 exit. It uses the supplied `Bylarm 2.txt` playlist and deliberately avoids solving or bypassing browser challenges.

## Import sample

- Rekordbox TXT export encoded as UTF-16 LE with tab-separated fields.
- 170 data rows and 10 source columns.
- 158 rows contain both artist and title; 12 cannot be discovery seeds without correction.
- 31 titles contain a recognized trailing version marker. The importer separates conservative markers such as `Extended Mix` and named remixes while preserving uncertain parentheses in the title.
- Every original row and field is retained in SQLite alongside parsed identity fields.

## 1001Tracklists search coverage

The spike searched an evenly spaced sample of 20 eligible rows through installed Chrome, with at most 10 results per query and conservative pacing.

| Classification | Count | Meaning |
| --- | ---: | --- |
| Strict match | 4 | Artist, title, and known version agreed under the spike's conservative normalization. |
| Representation mismatch worth review | 2 | A plausible result existed but artist credits differed in ordering or featured-artist notation. |
| No acceptable exact candidate | 14 | The query did not produce a result safe enough to accept automatically. |

Strict examples included Chlär — Dopamine Rush, Special Request — Curtain Twitcher (Nina Kraviz Alice Was Here Remix), carnidork — ragamuffin, and ♥ GOJII ♥ — DESIIRE. Plausible representation mismatches included Skrillex/ISOxo — Fuze and Rihanna/Calvin Harris — We Found Love.

The sample also exposes metadata that needs manual review rather than aggressive normalization: some title cells contain another artist and title, and some rows appear to name an edit author as the artist. This supports progressive queries and explicit review, but not permissive automatic matching.

## Access behavior

- Public search-result pages loaded and yielded structured track links in headless installed Chrome.
- A direct track-detail navigation returned a Cloudflare Turnstile forwarding page (HTTP 206 in the observed session).
- The adapter reports this as `BROWSER_CHALLENGE`; it does not turn the page into an empty result and does not attempt to solve the challenge.
- Appearance and tracklist coverage therefore remains unproven. The implementation must pause the source queue and let the user continue in the same dedicated, visible Chrome profile.

## Packaging and playback

- The debug Tauri macOS application bundle builds successfully with a self-contained arm64 Node/Playwright sidecar. The bundle is approximately 99 MB and does not need a development Node installation or a Playwright-managed browser cache.
- A sidecar executed from inside the finished `.app` returned a structured live search result. The complete React → Rust → bundled sidecar → installed Chrome path was also exercised from the match-review UI.
- Packaging required pinning Playwright 1.55.0. A caret range had silently selected 1.63, whose inspector dependency is incompatible with the selected `pkg` runtime. Playwright's `browsers.json` also had to be declared as a package asset, and serializable page callbacks were replaced with locator reads.
- MPH — Raw (Extended Mix) demonstrated progressive-query behavior: the versioned query surfaced a wrong remix, while the artist/title query surfaced an unversioned `MPH - Raw` page. Neither is safe to auto-confirm as the Extended Mix without additional evidence.
- Production YouTube playback is not yet tested because no YouTube API key or accepted external-playback fallback has been supplied.

## Gate decision

Do not start the crawler or recommendation phases yet. Search coverage and sidecar packaging are useful enough to keep testing, but the discovery-to-evidence path has not passed: track-detail access is currently challenged and playback remains unverified.

The visible-browser challenge pause/resume increment is now implemented: match review can open a track page in the dedicated Chrome profile, wait for normal user interaction for up to three minutes, and resume extraction automatically. It still needs a live verification against the observed Turnstile page. If detail pages remain inaccessible after normal user interaction, reassess user-supplied evidence URLs or the 1001Tracklists integration itself before investing in durable crawling.

Phase 2 queue groundwork has started without crossing that gate: the app can persist a bounded run and one idempotent appearance job per unique confirmed source track, with pause, resume, and cancel state transitions. Preparing this queue performs no source requests; live job execution remains disabled until access is verified.
