ALTER TABLE jobs ADD COLUMN result_json TEXT;

CREATE TABLE saved_tracks (
  source_track_id INTEGER PRIMARY KEY REFERENCES source_tracks(id) ON DELETE CASCADE,
  saved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE global_dismissals (
  source_track_id INTEGER PRIMARY KEY REFERENCES source_tracks(id) ON DELETE CASCADE,
  dismissed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_page_cache_expiry ON page_cache(cache_key, expires_at);
