ALTER TABLE discovery_runs ADD COLUMN message TEXT;

ALTER TABLE jobs ADD COLUMN created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP;
ALTER TABLE jobs ADD COLUMN updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP;
ALTER TABLE jobs ADD COLUMN available_at TEXT;

CREATE INDEX idx_runs_import_created ON discovery_runs(import_id, created_at DESC);
CREATE INDEX idx_jobs_run_kind_status ON jobs(run_id, kind, status);
