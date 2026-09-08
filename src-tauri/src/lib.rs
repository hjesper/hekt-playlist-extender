mod parser;

use parser::{parse_playlist, ImportPreview};
use serde::Serialize;
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::{
    path::PathBuf,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::Manager;
use tauri_plugin_shell::ShellExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LibrarySummary {
    imports: i64,
    tracks: i64,
    selected_seeds: i64,
    pending_seeds: i64,
    accepted_seeds: i64,
    skipped_seeds: i64,
    import_name: Option<String>,
}

const DISCOVERY_SETTINGS_VERSION: &str = "bounded-v1";
const DISCOVERY_ADAPTER_VERSION: &str = "1001tracklists-v1";
const RANKING_VERSION: &str = "cooccurrence-v1";
const MAX_APPEARANCES_PER_SEED: i64 = 25;
const MAX_TRACKLISTS_PER_RUN: i64 = 100;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryRunSummary {
    id: i64,
    status: String,
    stage: String,
    message: Option<String>,
    queued_jobs: i64,
    running_jobs: i64,
    completed_jobs: i64,
    failed_jobs: i64,
    total_jobs: i64,
    max_appearances_per_seed: i64,
    max_tracklists: i64,
    created_at: String,
    updated_at: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceSearchInput {
    artist: String,
    title: String,
    version: Option<String>,
    limit: Option<u8>,
}

static SOURCE_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

async fn run_source_adapter(
    app: &tauri::AppHandle,
    operation: &str,
    payload: serde_json::Value,
    timeout_ms: u64,
    visible: bool,
) -> Result<serde_json::Value, String> {
    let profile = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not locate application data: {error}"))?
        .join("browser-profile");
    std::fs::create_dir_all(&profile)
        .map_err(|error| format!("Could not create the dedicated browser profile: {error}"))?;
    let request = serde_json::json!({
        "version": 1,
        "requestId": format!("ui-{}-{}", std::process::id(), SOURCE_REQUEST_ID.fetch_add(1, Ordering::Relaxed)),
        "operation": operation,
        "payload": payload,
        "timeoutMs": timeout_ms
    });
    let output = app
        .shell()
        .sidecar("hekt-source-adapter")
        .map_err(|error| format!("Could not prepare source adapter: {error}"))?
        .env("HEKT_BROWSER_PROFILE", profile)
        .env("HEKT_BROWSER_HEADLESS", if visible { "0" } else { "1" })
        .arg(request.to_string())
        .output()
        .await
        .map_err(|error| format!("Could not run source adapter: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Source adapter failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Source adapter returned invalid data: {error}"))?;
    if response.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        let code = response
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("SOURCE_ERROR");
        let message = response
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Source request failed");
        return Err(format!("{code}: {message}"));
    }
    Ok(response
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

#[tauri::command]
async fn search_source(
    app: tauri::AppHandle,
    input: SourceSearchInput,
) -> Result<serde_json::Value, String> {
    let artist = input.artist.trim();
    let title = input.title.trim();
    if artist.is_empty() || title.is_empty() {
        return Err("Artist and title are required for source search".into());
    }
    if artist.len() > 300
        || title.len() > 500
        || input
            .version
            .as_deref()
            .is_some_and(|value| value.len() > 300)
    {
        return Err("Source search fields are too long".into());
    }
    let limit = input.limit.unwrap_or(10);
    if !(1..=25).contains(&limit) {
        return Err("Source search limit must be between 1 and 25".into());
    }
    run_source_adapter(
        &app,
        "searchTracks",
        serde_json::json!({ "artist": artist, "title": title, "version": input.version, "limit": limit }),
        45_000,
        false,
    )
    .await
}

#[tauri::command]
async fn verify_source_track(
    app: tauri::AppHandle,
    url: String,
) -> Result<serde_json::Value, String> {
    run_source_adapter(
        &app,
        "fetchTrack",
        serde_json::json!({
            "url": url,
            "interactive": true,
            "challengeTimeoutMs": 180_000
        }),
        240_000,
        true,
    )
    .await
}

#[tauri::command]
async fn library_summary(db: tauri::State<'_, SqlitePool>) -> Result<LibrarySummary, String> {
    let row = sqlx::query(
        "WITH latest AS (SELECT id, name FROM imports ORDER BY id DESC LIMIT 1)
         SELECT
           (SELECT count(*) FROM imports),
           (SELECT count(*) FROM import_rows WHERE import_id = (SELECT id FROM latest)),
           (SELECT count(*) FROM import_rows WHERE import_id = (SELECT id FROM latest) AND selected_seed = 1),
           (SELECT count(*) FROM seed_matches sm JOIN import_rows ir ON ir.id = sm.import_row_id WHERE ir.import_id = (SELECT id FROM latest) AND ir.selected_seed = 1 AND sm.status = 'pending'),
           (SELECT count(*) FROM seed_matches sm JOIN import_rows ir ON ir.id = sm.import_row_id WHERE ir.import_id = (SELECT id FROM latest) AND ir.selected_seed = 1 AND sm.status = 'accepted'),
           (SELECT count(*) FROM seed_matches sm JOIN import_rows ir ON ir.id = sm.import_row_id WHERE ir.import_id = (SELECT id FROM latest) AND ir.selected_seed = 1 AND sm.status = 'skipped'),
           (SELECT name FROM latest)",
    )
        .fetch_one(db.inner()).await.map_err(|e| e.to_string())?;
    Ok(LibrarySummary {
        imports: row.get(0),
        tracks: row.get(1),
        selected_seeds: row.get(2),
        pending_seeds: row.get(3),
        accepted_seeds: row.get(4),
        skipped_seeds: row.get(5),
        import_name: row.get(6),
    })
}

async fn discovery_run_summary(
    db: &SqlitePool,
    run_id: Option<i64>,
) -> Result<Option<DiscoveryRunSummary>, String> {
    let row = sqlx::query(
        "SELECT dr.id, dr.status, dr.stage, dr.message, dr.settings_json,
                dr.created_at, dr.updated_at,
                sum(CASE WHEN j.status = 'queued' THEN 1 ELSE 0 END),
                sum(CASE WHEN j.status = 'running' THEN 1 ELSE 0 END),
                sum(CASE WHEN j.status = 'completed' THEN 1 ELSE 0 END),
                sum(CASE WHEN j.status = 'failed' THEN 1 ELSE 0 END),
                count(j.id)
         FROM discovery_runs dr
         LEFT JOIN jobs j ON j.run_id = dr.id
         WHERE dr.id = COALESCE(?, (
           SELECT dr2.id FROM discovery_runs dr2
           WHERE dr2.import_id = (SELECT id FROM imports ORDER BY id DESC LIMIT 1)
           ORDER BY dr2.id DESC LIMIT 1
         ))
         GROUP BY dr.id",
    )
    .bind(run_id)
    .fetch_optional(db)
    .await
    .map_err(|e| e.to_string())?;
    let Some(row) = row else { return Ok(None) };
    let settings: serde_json::Value = serde_json::from_str(row.get::<String, _>(4).as_str())
        .map_err(|e| format!("Stored discovery settings are invalid: {e}"))?;
    Ok(Some(DiscoveryRunSummary {
        id: row.get(0),
        status: row.get(1),
        stage: row.get(2),
        message: row.get(3),
        max_appearances_per_seed: settings
            .get("maxAppearancesPerSeed")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(MAX_APPEARANCES_PER_SEED),
        max_tracklists: settings
            .get("maxTracklists")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(MAX_TRACKLISTS_PER_RUN),
        created_at: row.get(5),
        updated_at: row.get(6),
        queued_jobs: row.get(7),
        running_jobs: row.get(8),
        completed_jobs: row.get(9),
        failed_jobs: row.get(10),
        total_jobs: row.get(11),
    }))
}

async fn create_discovery_run_record(db: &SqlitePool) -> Result<DiscoveryRunSummary, String> {
    let mut tx = db.begin().await.map_err(|e| e.to_string())?;
    let import_id: i64 = sqlx::query_scalar("SELECT id FROM imports ORDER BY id DESC LIMIT 1")
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Import a playlist before preparing discovery".to_string())?;

    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM seed_matches sm
         JOIN import_rows ir ON ir.id = sm.import_row_id
         WHERE ir.import_id=? AND ir.selected_seed=1 AND sm.status='pending'",
    )
    .bind(import_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if pending > 0 {
        return Err("Finish reviewing every selected seed before preparing discovery".into());
    }

    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM discovery_runs
         WHERE import_id=? AND status IN ('queued','running','waiting_for_review','waiting_for_browser','paused')
         ORDER BY id DESC LIMIT 1",
    )
    .bind(import_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if existing.is_some() {
        return Err("This playlist already has an active discovery run".into());
    }

    let seeds = sqlx::query(
        "SELECT min(ir.id), st.id, st.provider_id, st.url
         FROM seed_matches sm
         JOIN import_rows ir ON ir.id = sm.import_row_id
         JOIN source_tracks st ON st.id = sm.source_track_id
         WHERE ir.import_id=? AND ir.selected_seed=1 AND sm.status='accepted'
         GROUP BY st.id, st.provider_id, st.url
         ORDER BY min(ir.row_number)",
    )
    .bind(import_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if seeds.is_empty() {
        return Err("Confirm at least one exact seed match before preparing discovery".into());
    }
    if seeds.len() > 20 {
        return Err("A discovery run can use at most 20 seeds".into());
    }

    let settings = serde_json::json!({
        "version": DISCOVERY_SETTINGS_VERSION,
        "maxSeeds": 20,
        "maxAppearancesPerSeed": MAX_APPEARANCES_PER_SEED,
        "maxTracklists": MAX_TRACKLISTS_PER_RUN,
        "selectionPolicy": "round-robin-seeds-v1"
    });
    let run_id: i64 = sqlx::query(
        "INSERT INTO discovery_runs(import_id,status,stage,settings_json,adapter_version,ranking_version,message)
         VALUES(?,'queued','fetching_appearances',?,?,?,?) RETURNING id",
    )
    .bind(import_id)
    .bind(settings.to_string())
    .bind(DISCOVERY_ADAPTER_VERSION)
    .bind(RANKING_VERSION)
    .bind("Queue prepared. Live source execution remains gated until access verification passes.")
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?
    .get(0);

    for (seed_order, seed) in seeds.into_iter().enumerate() {
        let import_row_id: i64 = seed.get(0);
        let source_track_id: i64 = seed.get(1);
        let provider_id: String = seed.get(2);
        let source_url: String = seed.get(3);
        let payload = serde_json::json!({
            "seedOrder": seed_order,
            "importRowId": import_row_id,
            "sourceTrackId": source_track_id,
            "sourceUrl": source_url,
            "appearanceLimit": MAX_APPEARANCES_PER_SEED
        });
        sqlx::query(
            "INSERT INTO jobs(run_id,job_key,kind,status,payload_json,created_at,updated_at)
             VALUES(?,?,'fetch_appearances','queued',?,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",
        )
        .bind(run_id)
        .bind(format!("appearances:{provider_id}"))
        .bind(payload.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    discovery_run_summary(db, Some(run_id))
        .await?
        .ok_or_else(|| "The discovery run could not be loaded after creation".to_string())
}

async fn update_discovery_run_status(
    db: &SqlitePool,
    run_id: i64,
    action: &str,
) -> Result<DiscoveryRunSummary, String> {
    let (from, status, message) = match action {
        "pause" => (
            vec!["queued", "running", "waiting_for_browser"],
            "paused",
            "Run paused. Completed source work will be retained.",
        ),
        "resume" => (
            vec!["paused", "waiting_for_browser"],
            "queued",
            "Run queued to resume from persisted work.",
        ),
        "cancel" => (
            vec!["queued", "running", "waiting_for_browser", "paused"],
            "cancelled",
            "Run cancelled. Completed source work has been retained.",
        ),
        _ => return Err("Discovery action must be pause, resume, or cancel".into()),
    };
    let current: Option<String> =
        sqlx::query_scalar("SELECT status FROM discovery_runs WHERE id=?")
            .bind(run_id)
            .fetch_optional(db)
            .await
            .map_err(|e| e.to_string())?;
    let current = current.ok_or_else(|| "Discovery run not found".to_string())?;
    if !from.contains(&current.as_str()) {
        return Err(format!(
            "Cannot {action} a discovery run in {current} status"
        ));
    }
    let mut tx = db.begin().await.map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE discovery_runs SET status=?, message=?, updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(status)
    .bind(message)
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if action == "cancel" {
        sqlx::query(
            "UPDATE jobs SET status='cancelled', updated_at=CURRENT_TIMESTAMP
             WHERE run_id=? AND status IN ('queued','running')",
        )
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    } else if action == "pause" {
        sqlx::query(
            "UPDATE jobs SET status='queued', updated_at=CURRENT_TIMESTAMP
             WHERE run_id=? AND status='running'",
        )
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    discovery_run_summary(db, Some(run_id))
        .await?
        .ok_or_else(|| "Discovery run not found".to_string())
}

#[tauri::command]
async fn latest_discovery_run(
    db: tauri::State<'_, SqlitePool>,
) -> Result<Option<DiscoveryRunSummary>, String> {
    discovery_run_summary(db.inner(), None).await
}

#[tauri::command]
async fn start_discovery(db: tauri::State<'_, SqlitePool>) -> Result<DiscoveryRunSummary, String> {
    create_discovery_run_record(db.inner()).await
}

#[tauri::command]
async fn control_discovery(
    run_id: i64,
    action: String,
    db: tauri::State<'_, SqlitePool>,
) -> Result<DiscoveryRunSummary, String> {
    update_discovery_run_status(db.inner(), run_id, &action).await
}

#[tauri::command]
async fn list_tracks(db: tauri::State<'_, SqlitePool>) -> Result<Vec<parser::Track>, String> {
    let rows = sqlx::query(
        "SELECT ir.id, ir.row_number, ir.artist, ir.title, ir.version, ir.label, ir.bpm,
                ir.musical_key, ir.duration, ir.selected_seed, sm.status, st.url
         FROM import_rows ir
         LEFT JOIN seed_matches sm ON sm.import_row_id = ir.id
         LEFT JOIN source_tracks st ON st.id = sm.source_track_id
         WHERE ir.import_id = (SELECT id FROM imports ORDER BY id DESC LIMIT 1)
         ORDER BY ir.row_number",
    )
    .fetch_all(db.inner())
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| parser::Track {
            id: r.get(0),
            row_number: r.get(1),
            artist: r.get(2),
            title: r.get(3),
            version: r.get(4),
            label: r.get(5),
            bpm: r.get(6),
            key: r.get(7),
            duration: r.get(8),
            selected: r.get::<i64, _>(9) == 1,
            match_status: r.get(10),
            source_url: r.get(11),
            original_fields: Default::default(),
        })
        .collect())
}

#[tauri::command]
async fn preview_playlist(path: String) -> Result<ImportPreview, String> {
    parse_playlist(PathBuf::from(path).as_path(), "Preview").map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_playlist(
    path: String,
    name: String,
    db: tauri::State<'_, SqlitePool>,
) -> Result<ImportPreview, String> {
    let mut preview =
        parse_playlist(PathBuf::from(&path).as_path(), &name).map_err(|e| e.to_string())?;
    let mut tx = db.begin().await.map_err(|e| e.to_string())?;
    let import_id = sqlx::query("INSERT INTO imports(name, source_path, encoding, delimiter) VALUES(?, ?, ?, ?) RETURNING id")
        .bind(&name).bind(&path).bind(&preview.encoding).bind(&preview.delimiter).fetch_one(&mut *tx).await.map_err(|e| e.to_string())?.get::<i64,_>(0);
    for track in &mut preview.tracks {
        let raw = serde_json::to_string(&track.original_fields).map_err(|e| e.to_string())?;
        let inserted = sqlx::query("INSERT INTO import_rows(import_id,row_number,artist,title,version,label,bpm,musical_key,duration,original_json) VALUES(?,?,?,?,?,?,?,?,?,?) RETURNING id")
            .bind(import_id).bind(track.row_number).bind(&track.artist).bind(&track.title).bind(&track.version).bind(&track.label).bind(track.bpm).bind(&track.key).bind(&track.duration).bind(raw)
            .fetch_one(&mut *tx).await.map_err(|e| e.to_string())?;
        track.id = inserted.get(0);
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(preview)
}

#[tauri::command]
async fn set_seed(
    track_id: i64,
    selected: bool,
    db: tauri::State<'_, SqlitePool>,
) -> Result<(), String> {
    let import_id: Option<i64> = sqlx::query_scalar("SELECT import_id FROM import_rows WHERE id=?")
        .bind(track_id)
        .fetch_optional(db.inner())
        .await
        .map_err(|e| e.to_string())?;
    let import_id = import_id.ok_or_else(|| "Track not found".to_string())?;
    if selected {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM import_rows WHERE import_id=? AND selected_seed=1",
        )
        .bind(import_id)
        .fetch_one(db.inner())
        .await
        .map_err(|e| e.to_string())?;
        if count >= 20 {
            return Err("A discovery run can use at most 20 seeds".into());
        }
    }
    sqlx::query("UPDATE import_rows SET selected_seed=? WHERE id=?")
        .bind(selected)
        .bind(track_id)
        .execute(db.inner())
        .await
        .map_err(|e| e.to_string())?;
    if selected {
        sqlx::query(
            "INSERT INTO seed_matches(import_row_id,status) VALUES(?,'pending')
             ON CONFLICT(import_row_id) DO NOTHING",
        )
        .bind(track_id)
        .execute(db.inner())
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn resolve_seed(
    track_id: i64,
    status: String,
    source_url: Option<String>,
    db: tauri::State<'_, SqlitePool>,
) -> Result<(), String> {
    if !matches!(status.as_str(), "pending" | "accepted" | "skipped") {
        return Err("Seed status must be pending, accepted, or skipped".into());
    }
    let track =
        sqlx::query("SELECT artist, title, version, selected_seed FROM import_rows WHERE id=?")
            .bind(track_id)
            .fetch_optional(db.inner())
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?;
    if track.get::<i64, _>(3) != 1 {
        return Err("Select the track as a seed before resolving its match".into());
    }

    let source_track_id = if status == "accepted" {
        let raw_url = source_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Paste the matching 1001Tracklists track URL".to_string())?;
        let url =
            tauri::Url::parse(raw_url).map_err(|_| "The source URL is invalid".to_string())?;
        let host = url.host_str().unwrap_or_default();
        if url.scheme() != "https" || host != "www.1001tracklists.com" {
            return Err("Use an HTTPS track URL on www.1001tracklists.com".into());
        }
        let provider_id = url
            .path_segments()
            .and_then(|mut segments| {
                (segments.next() == Some("track"))
                    .then(|| segments.next().filter(|value| !value.is_empty()))
                    .flatten()
            })
            .ok_or_else(|| {
                "The source URL must identify a 1001Tracklists track page".to_string()
            })?;
        Some(
            sqlx::query(
                "INSERT INTO source_tracks(provider,provider_id,artist,title,version,adapter_version,url)
                 VALUES('1001tracklists',?,?,?,?, 'manual-v1', ?)
                 ON CONFLICT(provider,provider_id) DO UPDATE SET
                   artist=excluded.artist, title=excluded.title, version=excluded.version, url=excluded.url
                 RETURNING id",
            )
            .bind(provider_id)
            .bind(track.get::<String, _>(0))
            .bind(track.get::<String, _>(1))
            .bind(track.get::<Option<String>, _>(2))
            .bind(url.as_str())
            .fetch_one(db.inner())
            .await
            .map_err(|e| e.to_string())?
            .get::<i64, _>(0),
        )
    } else {
        None
    };

    sqlx::query(
        "INSERT INTO seed_matches(import_row_id,source_track_id,status,matched_manually,updated_at)
         VALUES(?,?,?,?,CURRENT_TIMESTAMP)
         ON CONFLICT(import_row_id) DO UPDATE SET
           source_track_id=excluded.source_track_id,
           status=excluded.status,
           matched_manually=excluded.matched_manually,
           updated_at=CURRENT_TIMESTAMP",
    )
    .bind(track_id)
    .bind(source_track_id)
    .bind(&status)
    .bind(status == "accepted")
    .execute(db.inner())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let options = SqliteConnectOptions::from_str(&format!(
                "sqlite://{}",
                data_dir.join("hekt.db").display()
            ))?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
            let pool = tauri::async_runtime::block_on(SqlitePool::connect_with(options))?;
            tauri::async_runtime::block_on(sqlx::migrate!("./migrations").run(&pool))?;
            app.manage(pool);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library_summary,
            list_tracks,
            preview_playlist,
            import_playlist,
            set_seed,
            resolve_seed,
            search_source,
            verify_source_track,
            latest_discovery_run,
            start_discovery,
            control_discovery
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hekt");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_pool() -> (TempDir, SqlitePool) {
        let directory = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::from_str(&format!(
            "sqlite://{}",
            directory.path().join("test.db").display()
        ))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        (directory, pool)
    }

    async fn add_accepted_seed(pool: &SqlitePool, import_id: i64, row_number: i64) {
        let row_id: i64 = sqlx::query(
            "INSERT INTO import_rows(import_id,row_number,artist,title,original_json,selected_seed)
             VALUES(?,?,?,?,'{}',1) RETURNING id",
        )
        .bind(import_id)
        .bind(row_number)
        .bind(format!("Artist {row_number}"))
        .bind(format!("Title {row_number}"))
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
        let provider_id = format!("track-{row_number}");
        let source_id: i64 = sqlx::query(
            "INSERT INTO source_tracks(provider,provider_id,artist,title,adapter_version,url)
             VALUES('1001tracklists',?,?,?,'manual-v1',?) RETURNING id",
        )
        .bind(&provider_id)
        .bind(format!("Artist {row_number}"))
        .bind(format!("Title {row_number}"))
        .bind(format!(
            "https://www.1001tracklists.com/track/{provider_id}/title.html"
        ))
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
        sqlx::query(
            "INSERT INTO seed_matches(import_row_id,source_track_id,status,matched_manually)
             VALUES(?,?,'accepted',1)",
        )
        .bind(row_id)
        .bind(source_id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[test]
    fn creates_an_idempotent_bounded_queue_and_controls_its_lifecycle() {
        tauri::async_runtime::block_on(async {
            let (_directory, pool) = test_pool().await;
            let import_id: i64 = sqlx::query(
                "INSERT INTO imports(name,source_path,encoding,delimiter)
                 VALUES('Test','/test.txt','UTF-8','tab') RETURNING id",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
            add_accepted_seed(&pool, import_id, 1).await;
            add_accepted_seed(&pool, import_id, 2).await;

            let run = create_discovery_run_record(&pool).await.unwrap();
            assert_eq!(run.status, "queued");
            assert_eq!(run.stage, "fetching_appearances");
            assert_eq!(run.total_jobs, 2);
            assert_eq!(run.queued_jobs, 2);
            assert_eq!(run.max_appearances_per_seed, 25);
            assert_eq!(run.max_tracklists, 100);

            let keys: Vec<String> = sqlx::query_scalar(
                "SELECT job_key FROM jobs WHERE run_id=? ORDER BY json_extract(payload_json,'$.seedOrder')",
            )
            .bind(run.id)
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(keys, vec!["appearances:track-1", "appearances:track-2"]);
            assert!(create_discovery_run_record(&pool)
                .await
                .unwrap_err()
                .contains("active discovery run"));

            let paused = update_discovery_run_status(&pool, run.id, "pause")
                .await
                .unwrap();
            assert_eq!(paused.status, "paused");
            let resumed = update_discovery_run_status(&pool, run.id, "resume")
                .await
                .unwrap();
            assert_eq!(resumed.status, "queued");
            let cancelled = update_discovery_run_status(&pool, run.id, "cancel")
                .await
                .unwrap();
            assert_eq!(cancelled.status, "cancelled");
            let cancelled_jobs: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM jobs WHERE run_id=? AND status='cancelled'",
            )
            .bind(run.id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(cancelled_jobs, 2);

            let replacement = create_discovery_run_record(&pool).await.unwrap();
            assert_ne!(replacement.id, run.id);
            assert_eq!(replacement.total_jobs, 2);
        });
    }
}
