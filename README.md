# Hekt Playlist Extender

A local-first macOS desktop app that turns a Rekordbox TXT export into an evidence-backed audition shortlist.

## Development

Requirements: Node 20+, pnpm, Rust, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm test
pnpm build
pnpm tauri dev
```

The current milestone implements the import and seed-review foundation plus a bounded Phase 0 source adapter:

- UTF-8/UTF-16 Rekordbox TXT, TSV, and CSV parsing with an import preview
- a migration-backed local SQLite library using WAL and foreign keys
- per-playlist selection of up to 20 discovery seeds
- persistent manual match confirmation by 1001Tracklists URL, or explicit skip
- a versioned, bounded JSON Lines protocol and installed-Chrome Playwright sidecar
- conservative track search and validated detail-page navigation
- explicit `BROWSER_CHALLENGE` responses for challenge/forwarding pages
- a visible Chrome handoff that waits for normal user interaction and resumes track-page extraction in the same persistent profile
- persisted bounded discovery runs with one idempotent appearance job per unique confirmed source track, plus pause, resume, and cancel lifecycle controls

Playlist data stays local. The live source experiment sends only selected artist/title/version search terms to 1001Tracklists. Crawling, YouTube playback, recommendation actions, and CSV export remain intentionally disabled until the Phase 0 access, packaging, and playback gate passes. See [the current spike findings](docs/phase-0-spike.md).

To repeat the bounded search-coverage experiment with installed Chrome:

```sh
HEKT_BROWSER_HEADLESS=1 pnpm --filter @hekt/source-adapter spike:coverage -- /path/to/rekordbox.txt 20
```

Normal source searches use installed Chrome in the background. Set `HEKT_BROWSER_HEADLESS=0` only when deliberately testing a visible browser session or challenge handoff.

To build only the macOS app bundle while iterating locally:

```sh
pnpm tauri build --debug --bundles app
```
