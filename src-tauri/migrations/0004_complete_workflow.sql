CREATE TABLE recommendation_evidence (
  id INTEGER PRIMARY KEY,
  recommendation_id INTEGER NOT NULL REFERENCES recommendations(id) ON DELETE CASCADE,
  tracklist_id INTEGER NOT NULL REFERENCES tracklists(id) ON DELETE CASCADE,
  seed_source_track_id INTEGER NOT NULL REFERENCES source_tracks(id),
  proximity INTEGER,
  UNIQUE(recommendation_id, tracklist_id, seed_source_track_id)
);

CREATE TABLE feedback (
  id INTEGER PRIMARY KEY,
  import_id INTEGER NOT NULL REFERENCES imports(id) ON DELETE CASCADE,
  source_track_id INTEGER NOT NULL REFERENCES source_tracks(id) ON DELETE CASCADE,
  disposition TEXT NOT NULL CHECK(disposition IN ('saved','rejected','dismissed')),
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(import_id, source_track_id)
);

CREATE TABLE audio_sources (
  id INTEGER PRIMARY KEY,
  source_track_id INTEGER NOT NULL REFERENCES source_tracks(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  url TEXT NOT NULL,
  identity_confidence TEXT NOT NULL DEFAULT 'manual',
  playback_status TEXT NOT NULL DEFAULT 'unknown',
  checked_at TEXT,
  preferred INTEGER NOT NULL DEFAULT 0 CHECK(preferred IN (0,1)),
  UNIQUE(source_track_id, provider, url)
);

CREATE INDEX idx_evidence_recommendation ON recommendation_evidence(recommendation_id);
CREATE INDEX idx_feedback_import ON feedback(import_id, disposition);
