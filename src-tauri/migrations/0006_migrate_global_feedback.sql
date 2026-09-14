INSERT OR IGNORE INTO saved_tracks(source_track_id, saved_at)
SELECT source_track_id, updated_at FROM feedback WHERE disposition='saved';

INSERT OR IGNORE INTO global_dismissals(source_track_id, dismissed_at)
SELECT source_track_id, updated_at FROM feedback WHERE disposition='dismissed';

DELETE FROM feedback WHERE disposition IN ('saved','dismissed');
