# PLAN.md implementation status

Checked: 2026-09-09.

## Implemented and covered locally

- UTF-8/UTF-16, tab/comma/semicolon import with original-row retention and parser fixtures.
- Up to 20 selected seeds, persistent manual exact-match confirmation, broad search, URL validation, and explicit skip/re-review.
- Versioned sidecar protocol, installed-Chrome profile, allowed-host checks, bounded timeouts, and explicit challenge errors.
- Durable appearance and tracklist jobs, one active headless request at a time, fair round-robin set selection, a 100-set cap, short/long cache lifetimes, explicit fresh runs, partial-result retention, retryable challenge resume, pause/cancel checkpoints, and restart reclamation. Visible Chrome is opened only through the explicit challenge-recovery action.
- Current-run-only deterministic ranking with long-set and repeated-DJ reduction, playlist/seed/global-dismiss exclusions, persisted component values, stable tie-breaking, and inspectable evidence URLs.
- Global saves, playlist rejects, global dismissals, undo, provider-validated manual audio attachment/replacement, wrong-version marking, external open, a persistent YouTube embed, and UTF-8 metadata CSV export.
- A debug macOS `.app` bundle containing the sidecar builds and launches successfully, preserves the existing library, applies the new migrations, and opens the native CSV save panel.

## Still needs live or packaged verification

- Complete a challenged track-detail handoff in the dedicated Chrome profile and verify appearance pagination and current 1001Tracklists tracklist markup against several real pages.
- Judge matching precision, extracted metadata, ranking usefulness, and whether approximately 30 candidates are available for the supplied real playlist.
- Verify the YouTube embed and failed-player fallback in the production Tauri WebView.
- Repeat the packaged macOS import-to-export smoke test after the complete-workflow changes, including restart recovery and absent Chrome states.

These are external validation gates, not claims that can be established by unit tests alone.

## Not implemented from the broader plan

- Editable import delimiter/column mapping when detection is uncertain.
- A separate canonical-track/alias model and automatic acceptance of only strong source matches. The current workflow deliberately requires manual confirmation.
- Negative-cache controls, diagnostic export/cleanup, and richer offline HTML fixtures for current source markup.
- YouTube Data API key storage, quota-aware search, automatic audio resolution, and Bandcamp/SoundCloud embed verification.
- Distribution work: multi-architecture packaging, signing/notarization, updates, and credential onboarding.

The deferred-work section of `PLAN.md` remains deferred.
