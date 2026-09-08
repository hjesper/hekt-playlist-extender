ALTER TABLE source_tracks ADD COLUMN url TEXT;

CREATE UNIQUE INDEX idx_seed_matches_import_row
ON seed_matches(import_row_id);
