# Ranking specification

Ranking version: `cooccurrence-v2`.

Only identified source tracks from tracklists completed for the current discovery run are eligible. Confirmed seeds, global dismissals, and playlist rows with the same conservatively normalized artist/title/version identity are excluded. Normalization lowercases Unicode alphanumeric characters and removes punctuation. A missing version remains `unknown`; it does not equal a named version.

For each candidate:

- `cooccurrence` is the sum of `1 / sqrt(tracklist length)` across supporting sets. A known DJ's second and later supporting sets contribute 65% to reduce repeated-DJ dominance. Sets with no DJ metadata receive no invented DJ identity and retain their normal set contribution.
- `seedCoverage` is the number of distinct confirmed seeds found with the candidate.
- `proximity` is the sum of `1 / (1 + nearest row distance)` for each supporting set.
- `djDiversity` is the number of distinct, normalized, non-empty DJ names.
- `adjacentCount` counts supporting sets whose nearest seed is no more than one row away.

The final score is:

`cooccurrence × 4 + seedCoverage × 2 + proximity × 2 + sqrt(djDiversity) × 0.5`

Candidates are ordered by score descending, then normalized artist, normalized title, and source-track ID. The ranking version, formula inputs, evidence tracklists, seed IDs, and proximity are persisted so an explanation can be reproduced.
