# Hekt Playlist Extender — recommended implementation plan

## Objective and starting assumptions

Build a local macOS application that turns a Rekordbox playlist TXT export into a useful shortlist of tracks to audition. Recommendations come from DJ sets containing confirmed playlist tracks, with evidence the user can inspect.

Start with a personal Mac installation, installed Google Chrome, and a user-supplied YouTube API key. These are planning assumptions, not confirmed user requirements. Revisit browser distribution, credential onboarding, supported architectures, and release signing before distributing the app to other people.

The first milestone is one complete workflow: import a real playlist, confirm seed matches, discover roughly 30 candidates when source coverage permits, audition them, save or reject them, and export a shortlist. An interrupted run must resume from persisted work.

## Technology and responsibilities

| Component | Decision |
|---|---|
| Desktop shell | Tauri 2 |
| Interface | React, TypeScript, Vite, Tailwind, and selected shadcn/ui components |
| Async UI state | TanStack Query for command results; React state for local interaction initially |
| Application core | Rust owns imports, database access, jobs, matching, ranking, and provider requests |
| Persistence | SQLite through sqlx, versioned migrations, foreign keys, and WAL |
| Website adapter | Node/TypeScript with Playwright, bundled as a sidecar |
| Browser | Installed Chrome with a dedicated application profile for the first version |
| Communication | Typed Tauri commands; versioned JSON Lines between Rust and the sidecar |
| Credentials | OS keychain, accessed through Rust |
| Workspace | pnpm for frontend and sidecar; Cargo for Rust |

Rust is the only database writer. The sidecar extracts structured source records and never decides canonical musical identity. The frontend invokes narrow commands and cannot run arbitrary shell commands. Sidecar stdout contains protocol messages only; diagnostics use stderr.

Keep a source-adapter boundary around 1001Tracklists. Its operations are search tracks, fetch track details, fetch paginated appearances, and fetch a tracklist. IPC messages carry request IDs, a protocol version, typed results/errors, and bounded timeouts. Validate response schemas and allowed destination hosts.

## Phase 0: prove access, packaging, playback, and usefulness

Timebox the initial spike to approximately 3–4 focused days. Produce a minimal packaged app and disposable experiments before building the full interface.

1. Parse one real Rekordbox TXT export and inspect its encoding, headers, and version conventions.
2. Use a representative selection of 10–20 tracks to assess 1001 coverage and wrong-version matches.
3. Search tracks, fetch appearances, and extract several different tracklists. Verify pagination, row ordering, unresolved IDs, and provider links.
4. Package the Node sidecar and launch it from the installed application without a development Node installation or development browser cache.
5. Exercise a dedicated Chrome profile, a manual challenge pause/resume, and browser restart.
6. Test YouTube playback and a failed-player fallback in the production application. First investigate the documented WebView referrer/app-identification approach. Use an isolated player-only loopback host only if needed and validated.
7. Build a small candidate list from the sample and manually inspect whether its musical versions and set evidence are useful.

Record match coverage, incorrect automatic matches, manual corrections, crawl interruptions, and audition quality. Do not force ambiguous matches to meet a coverage target.

Exit when the packaged app can execute the complete small discovery path, playback works or an explicitly chosen external-playback fallback is accepted, and the source provides useful evidence for the sample playlist.

If website access is unreliable, investigate user-supplied tracklist URLs or pause the integration decision; changing desktop shells does not fix source access. If sidecar packaging is the blocker, run a narrow Electron comparison. A player failure alone should trigger a player investigation before a full shell migration. Re-estimate the remaining work using the spike results.

## Phase 1: import, identity, and seed review

Build an import screen, a seed-selection/review screen, and the minimum persistent schema.

- Import the entire playlist. Detect UTF-8/UTF-16 and common delimiters, show a preview, and allow explicit corrections when detection is uncertain.
- Require artist and title mappings. Preserve all original rows and any available version, duration, label, BPM, and key fields.
- Select up to 20 tracks as discovery seeds. All imported tracks remain available for duplicate exclusion.
- Model a canonical track as a specific recording/version. Preserve Original, Extended, Radio Edit, and named remixes separately unless verified equivalent.
- Treat missing version metadata as unknown. Keep original display text alongside conservative matching fields; punctuation normalization alone never merges identities.
- Search with progressively broader queries. Automatically accept only strong artist/title/version agreement without conflicts; missing duration alone need not prevent a strong match.
- Let the user accept, skip, search again, or paste a source track URL. Manual matches are persistent and editable.
- When an accepted seed match changes, invalidate its derived evidence and rebuild affected recommendations using cached source data where possible.

Exit when a real playlist can be imported without silent row loss, ambiguous versions remain reviewable, and selected seeds are confirmed or explicitly skipped.

## Phase 2: bounded discovery and recovery

Implement the durable item queue before growing the crawler.

Defaults: at most 20 selected seeds, 25 appearances per seed, and 100 unique tracklists per run. Use one active page/navigation at a time with conservative pacing. These are application budgets, not claims about a published source rate limit.

Fetch appearance pages fairly across seeds and schedule tracklists with a deterministic policy that balances seed coverage and DJ diversity. Record the selection policy and settings on the run. Do not silently take the first 100 sets from the first few seeds.

Persist every successfully extracted page and its provenance. Cache reuse should consider source type, fetch time, and adapter version; completed old sets can be reused longer than changing search results or recent track pages. Provide explicit refresh and negative-cache expiry.

Keep the identified entries from every fetched set. Rank before limiting the displayed candidate list; do not discard later entries because an arbitrary first-1,000 threshold was reached. Apply separate technical page/response-size bounds for malformed inputs.

On a challenge or login screen, pause the source queue and offer to open the same dedicated browser session. Do not automate challenge solving. Repeated failures must produce an actionable paused/error state, not endless retries.

Use separate lifecycle status and current stage. Statuses include queued, running, waiting for review, waiting for browser, paused, completed, completed with errors, failed, and cancelled. Stages describe matching, fetching appearances, fetching tracklists, and ranking.

Pause finishes or times out the current item and stops scheduling. Cancel stops remaining work but retains completed data. App shutdown stops workers. On restart, reclaim interrupted work and offer resume. Unique job keys and transactional writes make retries safe. Do not run a background daemon in the first version.

Exit when an interrupted run resumes without duplicate records, blocked pages are not mistaken for empty sets, and individual failures preserve usable partial results.

## Phase 3: evidence and recommendation quality

Build candidates from identified tracklist entries. Store source track identity, version certainty, set identity, performer metadata, row position, and cue time where available. Unresolved IDs are retained as source entries but do not become recommendations.

Exclude tracks already present in the entire imported playlist, known duplicate recordings, and applicable dismissals. Keep uncertain identities separate for review. Detect duplicate representations of the same set where supported by metadata, rather than counting every URL as independent evidence.

Start with a deterministic ranking using weighted co-occurrence, distinct-seed coverage, and proximity. Reduce the contribution of long sets and repeated appearances by the same DJ. Document exact normalization, missing-data behavior, and stable tie-breaking in the implementation. Store the ranking version and component values so results can be reproduced and inspected.

Use missing metadata as unknown evidence. Do not invent DJs, dates, or cue times. Defer recency presets and novelty/popularity claims until the base ranking has been evaluated against real listening feedback.

Each result explains evidence from the fetched sample, for example: “Found in 7 fetched sets containing 3 of your seeds; 4 identified DJs; adjacent to a seed in 2 sets.” The evidence drawer links to those sets and shows the relevant seed/candidate entries. Any adjacency count must be computable from stored entries.

Saving is global. Rejecting hides a recording for the current imported playlist by default, with a separate global-dismiss option. Undo is available. Do not describe a candidate as unowned: the app only knows the imports supplied to it.

Exit when small golden graphs produce correct evidence and predictable ordering, and the sample playlist produces a shortlist worth auditioning. Tune ranking from observed failures before adding more features.

## Phase 4: auditioning, shortlist, and export

Discovery completes when recommendations are ranked. Audio resolution is a separate queue and never blocks discovery completion.

Reuse validated track-specific provider links from source pages and prior corrections before spending search quota. Do not attach a full DJ-set video as the individual track's source. Search YouTube on selection or for a small batch of top recommendations, with caching, a visible request budget, and explicit quota-exhausted state. Use video details where needed to assess duration and availability.

Store identity confidence and playback availability separately, with check timestamps. A playable URL may identify the wrong remix, and a correct remix may be unavailable. Playback status reflects the last observed result rather than a permanent guarantee.

Offer one persistent, visible provider player. Prefer YouTube, then a verified Bandcamp source, then a verified SoundCloud source. For the latter two, the first version supports known URLs, manual attachment, embeds where verified, and external search/open links. Defer automatic catalog search and SoundCloud credential setup.

Support source replacement, “wrong version,” provider switching, and open externally. Automatic fallback requires strong identity confidence. Use the player architecture proven in Phase 0. If a loopback player is required, bind only to loopback, isolate it from Tauri IPC, restrict its inputs, and validate cross-frame messages.

Export saved results as UTF-8 CSV containing artist, title/version, available label, preferred source URL, and evidence URLs. Describe this as a metadata shortlist, with no promise of Rekordbox import or access to playable local music files.

Exit when users can audition and correct sources, save/reject with undo, and export their shortlist even when some providers fail.

## Data model and interface scope

Start with imports/import rows, canonical tracks and aliases, source tracks, seed matches, tracklists/entries, discovery runs/jobs, recommendations/evidence, audio sources, feedback, and page cache. Derive track appearances from entries initially instead of maintaining a redundant writable relationship table.

Persist run settings, adapter/ranking versions, and enough evidence to reproduce explanations. Sanitized extraction fixtures are separate from browser profiles. Diagnostic export excludes credentials and browser sessions; failure artifacts have bounded retention and user-controlled cleanup.

Keep navigation small: Library, Import and Match Review, Discovery Results with progress and evidence, and Settings. Use a persistent audition player. Avoid building six elaborate screens before the first end-to-end flow works.

## Verification and release gate

- Parser fixtures cover actual exports, Unicode, delimiters, missing fields, and ambiguous versions.
- Offline HTML fixtures cover searches, track pages, pagination, identified/unresolved entries, challenges, and changed markup.
- Small golden graphs cover exclusions, duplicate sets, repeated DJs, ranking, and explanation counts.
- Recovery checks cover interruption during a fetch/write, retries, cancellation, and seed-match correction.
- A packaged macOS smoke test covers import through export, sidecar launch without developer dependencies, player success/failure, restart recovery, and absent Chrome/API-key states.

Assess usable recommendations, matching precision, and correction effort in addition to technical pass/fail. Source coverage may prevent any playlist from yielding 30 useful candidates; show that limitation instead of filling the list with weak matches.

Use a provisional budget of 4–7 focused developer weeks for this scoped version, revisited after Phase 0. Distribution to other users is a separate milestone covering installation, supported architectures, browser provisioning, signing/notarization, updates, and credential onboarding.

## Deferred work

Defer audio downloading, fingerprinting, BPM/key analysis, Rekordbox database modification, purported Rekordbox-compatible exports without validation, cloud sync, accounts, scheduled crawling, machine learning, popularity-based deep-cut modes, automated Bandcamp/SoundCloud catalog search, and cross-platform releases.

## Verified reference points

- [Tauri Node sidecars](https://v2.tauri.app/learn/sidecar-nodejs/): bundling Node is supported; the actual Playwright dependency bundle still needs validation.
- [Playwright browsers](https://playwright.dev/docs/browsers): installed Chrome is supported; managed browser binaries have version requirements.
- [Playwright Electron support](https://playwright.dev/docs/api/class-electron): Electron automation is experimental and should not be assumed to replace standard browser packaging automatically.
- [YouTube embedding requirements](https://developers.google.com/youtube/terms/required-minimum-functionality): verify desktop identification and visible-player requirements in the production integration.
- [YouTube search](https://developers.google.com/youtube/v3/docs/search/list): documentation checked during planning lists 100 search calls per day; treat actual project quota as configuration rather than a permanent constant.

1001Tracklists extraction details remain unverified implementation assumptions until the live spike passes.
