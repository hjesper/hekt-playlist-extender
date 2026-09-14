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

The current milestone implements the end-to-end local workflow in code:

- UTF-8/UTF-16 Rekordbox TXT, TSV, and CSV parsing with an import preview
- a migration-backed local SQLite library using WAL and foreign keys
- per-playlist selection of up to 20 discovery seeds
- persistent manual match confirmation by 1001Tracklists URL, or explicit skip
- a versioned, bounded JSON Lines protocol and installed-Chrome Playwright sidecar
- conservative track search and validated detail-page navigation
- explicit `BROWSER_CHALLENGE` responses for challenge/forwarding pages
- headless discovery by default, with a visible Chrome handoff only when the user explicitly opens a blocked challenge page
- persisted bounded discovery runs with one idempotent appearance job per unique confirmed source track, plus pause, resume, and cancel lifecycle controls
- cache-aware appearance and tracklist extraction with persisted partial results and actionable challenge states
- bounded round-robin set selection, restart recovery, and pause/cancel checkpoints between source items
- deterministic co-occurrence ranking, conservative playlist-wide exclusions, inspectable set evidence, and stable tie-breaking
- global saves and dismissals, per-playlist reject/undo feedback, validated manual playback sources, a persistent YouTube player, and UTF-8 shortlist CSV export

Playlist data, feedback, and cached evidence stay local. Source requests send only selected identity terms and URLs to 1001Tracklists. Browser challenges require normal user interaction and are never solved automatically. Playback uses URLs the user verifies and attaches; discovery does not spend YouTube API quota or block on audio resolution.

This is not yet a release-complete implementation of every item in `PLAN.md`. Live appearance/tracklist extraction after a normal challenge handoff, production WebView playback, recommendation usefulness, and the packaged import-to-export smoke test still require hands-on verification. Automatic strong-match acceptance, editable import column mapping, and YouTube API search/quota handling also remain outside the implemented path. See [the plan status](docs/plan-status.md), [ranking specification](docs/ranking.md), and [original spike findings](docs/phase-0-spike.md).

To repeat the bounded search-coverage experiment with installed Chrome:

```sh
HEKT_BROWSER_HEADLESS=1 pnpm --filter @hekt/source-adapter spike:coverage -- /path/to/rekordbox.txt 20
```

Normal source searches use installed Chrome in the background. Set `HEKT_BROWSER_HEADLESS=0` only when deliberately testing a visible browser session or challenge handoff.

To build only the macOS app bundle while iterating locally:

```sh
pnpm tauri build --debug --bundles app
```
