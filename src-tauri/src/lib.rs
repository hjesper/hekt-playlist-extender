mod parser;

use parser::{parse_playlist, ImportPreview};
use serde::Serialize;
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
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

const DISCOVERY_SETTINGS_VERSION: &str = "bounded-v2";
const DISCOVERY_ADAPTER_VERSION: &str = "1001tracklists-v1";
const RANKING_VERSION: &str = "cooccurrence-v2";
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

#[derive(Debug)]
struct SourceAdapterFailure {
    code: String,
    message: String,
    retryable: bool,
}

impl SourceAdapterFailure {
    fn needs_browser(&self) -> bool {
        matches!(
            self.code.as_str(),
            "BROWSER_CHALLENGE" | "BROWSER_CHALLENGE_TIMEOUT" | "BROWSER_SESSION_CLOSED"
        )
    }
}

impl fmt::Display for SourceAdapterFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

async fn run_source_adapter(
    app: &tauri::AppHandle,
    operation: &str,
    payload: serde_json::Value,
    timeout_ms: u64,
    visible: bool,
) -> Result<serde_json::Value, SourceAdapterFailure> {
    let profile = app
        .path()
        .app_data_dir()
        .map_err(|error| SourceAdapterFailure {
            code: "APP_DATA_ERROR".into(),
            message: format!("Could not locate application data: {error}"),
            retryable: false,
        })?
        .join("browser-profile");
    std::fs::create_dir_all(&profile).map_err(|error| SourceAdapterFailure {
        code: "BROWSER_PROFILE_ERROR".into(),
        message: format!("Could not create the dedicated browser profile: {error}"),
        retryable: false,
    })?;
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
        .map_err(|error| SourceAdapterFailure {
            code: "SIDECAR_START_ERROR".into(),
            message: format!("Could not prepare source adapter: {error}"),
            retryable: true,
        })?
        .env("HEKT_BROWSER_PROFILE", profile)
        .env("HEKT_BROWSER_HEADLESS", if visible { "0" } else { "1" })
        .arg(request.to_string())
        .output()
        .await
        .map_err(|error| SourceAdapterFailure {
            code: "SIDECAR_EXECUTION_ERROR".into(),
            message: format!("Could not run source adapter: {error}"),
            retryable: true,
        })?;
    if !output.status.success() {
        return Err(SourceAdapterFailure {
            code: "SIDECAR_EXIT_ERROR".into(),
            message: format!(
                "Source adapter failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            retryable: true,
        });
    }
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| SourceAdapterFailure {
            code: "SIDECAR_PROTOCOL_ERROR".into(),
            message: format!("Source adapter returned invalid data: {error}"),
            retryable: false,
        })?;
    if response.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        let code = response
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("SOURCE_ERROR");
        let message = response
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Source request failed");
        let retryable = response
            .pointer("/error/retryable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        return Err(SourceAdapterFailure {
            code: code.into(),
            message: message.into(),
            retryable,
        });
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
    .map_err(|error| error.to_string())
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
            "url": &url,
            "interactive": true,
            "challengeTimeoutMs": 180_000
        }),
        240_000,
        true,
    )
    .await
    .map_err(|error| error.to_string())
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

async fn create_discovery_run_record(
    db: &SqlitePool,
    refresh_source: bool,
) -> Result<DiscoveryRunSummary, String> {
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
        "selectionPolicy": "round-robin-seeds-v1",
        "refreshSource": refresh_source
    });
    let run_id: i64 = sqlx::query(
        "INSERT INTO discovery_runs(import_id,status,stage,settings_json,adapter_version,ranking_version,message)
         VALUES(?,'queued','fetching_appearances',?,?,?,?) RETURNING id",
    )
    .bind(import_id)
    .bind(settings.to_string())
    .bind(DISCOVERY_ADAPTER_VERSION)
    .bind(RANKING_VERSION)
    .bind(if refresh_source {
        "Fresh source queue prepared. Cached pages will be replaced."
    } else {
        "Queue prepared. Valid cached pages will be reused."
    })
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
    } else if action == "resume" {
        sqlx::query(
            "UPDATE jobs SET status='queued', updated_at=CURRENT_TIMESTAMP
             WHERE run_id=? AND status='failed' AND
               (last_error LIKE 'BROWSER_CHALLENGE:%'
                OR last_error LIKE 'BROWSER_CHALLENGE_TIMEOUT:%'
                OR last_error LIKE 'BROWSER_SESSION_CLOSED:%')",
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
async fn start_discovery(
    refresh_source: Option<bool>,
    db: tauri::State<'_, SqlitePool>,
) -> Result<DiscoveryRunSummary, String> {
    create_discovery_run_record(db.inner(), refresh_source.unwrap_or(false)).await
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecommendationView {
    id: i64,
    source_track_id: i64,
    artist: String,
    title: String,
    version: Option<String>,
    score: f64,
    set_count: i64,
    seed_count: i64,
    dj_count: i64,
    adjacent_count: i64,
    disposition: Option<String>,
    source_url: Option<String>,
    source_provider: Option<String>,
    playback_status: Option<String>,
    evidence_urls: Vec<String>,
}

#[tauri::command]
async fn list_recommendations(
    db: tauri::State<'_, SqlitePool>,
) -> Result<Vec<RecommendationView>, String> {
    let run_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM discovery_runs WHERE import_id=(SELECT id FROM imports ORDER BY id DESC LIMIT 1) ORDER BY id DESC LIMIT 1"
    ).fetch_optional(db.inner()).await.map_err(|e| e.to_string())?;
    let Some(run_id) = run_id else {
        return Ok(Vec::new());
    };
    let rows = sqlx::query(
        "SELECT r.id,r.source_track_id,st.artist,st.title,st.version,r.score,r.components_json,
                CASE
                  WHEN gd.source_track_id IS NOT NULL THEN 'dismissed'
                  WHEN f.disposition = 'rejected' THEN 'rejected'
                  WHEN s.source_track_id IS NOT NULL THEN 'saved'
                  ELSE NULL
                END,
                a.url,a.provider,a.playback_status
         FROM recommendations r JOIN source_tracks st ON st.id=r.source_track_id
         LEFT JOIN feedback f ON f.source_track_id=st.id AND f.import_id=(SELECT import_id FROM discovery_runs WHERE id=r.run_id)
         LEFT JOIN saved_tracks s ON s.source_track_id=st.id
         LEFT JOIN global_dismissals gd ON gd.source_track_id=st.id
         LEFT JOIN audio_sources a ON a.id=(SELECT id FROM audio_sources a2 WHERE a2.source_track_id=st.id AND a2.playback_status NOT IN ('wrong_version','unavailable') ORDER BY preferred DESC,id DESC LIMIT 1)
         WHERE r.run_id=? ORDER BY r.score DESC, lower(st.artist), lower(st.title), st.id"
    ).bind(run_id).fetch_all(db.inner()).await.map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let id: i64 = row.get(0);
        let components: serde_json::Value =
            serde_json::from_str(row.get::<String, _>(6).as_str()).unwrap_or_default();
        let evidence_urls = sqlx::query_scalar(
            "SELECT DISTINCT t.url FROM recommendation_evidence e JOIN tracklists t ON t.id=e.tracklist_id WHERE e.recommendation_id=? ORDER BY t.url"
        ).bind(id).fetch_all(db.inner()).await.map_err(|e| e.to_string())?;
        result.push(RecommendationView {
            id,
            source_track_id: row.get(1),
            artist: row.get(2),
            title: row.get(3),
            version: row.get(4),
            score: row.get(5),
            set_count: components
                .get("setCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            seed_count: components
                .get("seedCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            dj_count: components
                .get("djCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            adjacent_count: components
                .get("adjacentCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            disposition: row.get(7),
            source_url: row.get(8),
            source_provider: row.get(9),
            playback_status: row.get(10),
            evidence_urls,
        });
    }
    Ok(result)
}

#[tauri::command]
async fn set_recommendation_feedback(
    source_track_id: i64,
    disposition: Option<String>,
    db: tauri::State<'_, SqlitePool>,
) -> Result<(), String> {
    let import_id: i64 = sqlx::query_scalar("SELECT id FROM imports ORDER BY id DESC LIMIT 1")
        .fetch_optional(db.inner())
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No playlist imported")?;
    match disposition.as_deref() {
        None => {
            sqlx::query("DELETE FROM feedback WHERE import_id=? AND source_track_id=?")
                .bind(import_id)
                .bind(source_track_id)
                .execute(db.inner())
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM saved_tracks WHERE source_track_id=?")
                .bind(source_track_id)
                .execute(db.inner())
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM global_dismissals WHERE source_track_id=?")
                .bind(source_track_id)
                .execute(db.inner())
                .await
                .map_err(|e| e.to_string())?;
        }
        Some("saved") => {
            let mut tx = db.begin().await.map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM feedback WHERE import_id=? AND source_track_id=?")
                .bind(import_id)
                .bind(source_track_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM global_dismissals WHERE source_track_id=?")
                .bind(source_track_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query(
                "INSERT INTO saved_tracks(source_track_id,saved_at) VALUES(?,CURRENT_TIMESTAMP)
                 ON CONFLICT(source_track_id) DO UPDATE SET saved_at=CURRENT_TIMESTAMP",
            )
            .bind(source_track_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())?;
        }
        Some("rejected") => {
            let mut tx = db.begin().await.map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM saved_tracks WHERE source_track_id=?")
                .bind(source_track_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("INSERT INTO feedback(import_id,source_track_id,disposition,updated_at) VALUES(?,?,'rejected',CURRENT_TIMESTAMP) ON CONFLICT(import_id,source_track_id) DO UPDATE SET disposition='rejected',updated_at=CURRENT_TIMESTAMP")
                .bind(import_id)
                .bind(source_track_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())?;
        }
        Some("dismissed") => {
            let mut tx = db.begin().await.map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM saved_tracks WHERE source_track_id=?")
                .bind(source_track_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM feedback WHERE import_id=? AND source_track_id=?")
                .bind(import_id)
                .bind(source_track_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query(
                "INSERT INTO global_dismissals(source_track_id,dismissed_at) VALUES(?,CURRENT_TIMESTAMP)
                 ON CONFLICT(source_track_id) DO UPDATE SET dismissed_at=CURRENT_TIMESTAMP",
            )
            .bind(source_track_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())?;
        }
        _ => return Err("Disposition must be saved, rejected, dismissed, or null".into()),
    }
    Ok(())
}

#[tauri::command]
async fn attach_audio_source(
    source_track_id: i64,
    provider: String,
    url: String,
    db: tauri::State<'_, SqlitePool>,
) -> Result<(), String> {
    let parsed = tauri::Url::parse(url.trim()).map_err(|_| "Invalid audio URL")?;
    if parsed.scheme() != "https" {
        return Err("Audio sources must use HTTPS".into());
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let valid_host = match provider.as_str() {
        "youtube" => matches!(
            host.as_str(),
            "youtube.com" | "www.youtube.com" | "m.youtube.com" | "youtu.be"
        ),
        "soundcloud" => host == "soundcloud.com" || host.ends_with(".soundcloud.com"),
        "bandcamp" => host == "bandcamp.com" || host.ends_with(".bandcamp.com"),
        _ => return Err("Unsupported audio provider".into()),
    };
    if !valid_host {
        return Err(format!("That URL does not belong to {provider}"));
    }
    sqlx::query("UPDATE audio_sources SET preferred=0 WHERE source_track_id=?")
        .bind(source_track_id)
        .execute(db.inner())
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO audio_sources(source_track_id,provider,url,identity_confidence,playback_status,checked_at,preferred) VALUES(?,?,?,'manual','available',CURRENT_TIMESTAMP,1) ON CONFLICT(source_track_id,provider,url) DO UPDATE SET preferred=1,playback_status='available',checked_at=CURRENT_TIMESTAMP")
        .bind(source_track_id).bind(provider).bind(parsed.as_str()).execute(db.inner()).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn mark_audio_source_wrong_version(
    source_track_id: i64,
    db: tauri::State<'_, SqlitePool>,
) -> Result<(), String> {
    let changed = sqlx::query(
        "UPDATE audio_sources SET preferred=0,playback_status='wrong_version',checked_at=CURRENT_TIMESTAMP
         WHERE id=(SELECT id FROM audio_sources WHERE source_track_id=? AND preferred=1 ORDER BY id DESC LIMIT 1)",
    )
    .bind(source_track_id)
    .execute(db.inner())
    .await
    .map_err(|error| error.to_string())?
    .rows_affected();
    if changed == 0 {
        return Err("This recommendation has no preferred audio source".into());
    }
    Ok(())
}

async fn build_shortlist_csv(db: &SqlitePool) -> Result<String, String> {
    let rows = sqlx::query(
        "SELECT st.artist,st.title,st.version,a.url,st.id
         FROM recommendations r
         JOIN discovery_runs dr ON dr.id=r.run_id
         JOIN source_tracks st ON st.id=r.source_track_id
         JOIN saved_tracks s ON s.source_track_id=st.id
         LEFT JOIN audio_sources a ON a.id=(SELECT id FROM audio_sources a2 WHERE a2.source_track_id=st.id AND a2.playback_status NOT IN ('wrong_version','unavailable') ORDER BY preferred DESC,id DESC LIMIT 1)
         WHERE dr.import_id=(SELECT id FROM imports ORDER BY id DESC LIMIT 1)
           AND r.run_id=(SELECT id FROM discovery_runs WHERE import_id=dr.import_id ORDER BY id DESC LIMIT 1)
         ORDER BY lower(st.artist),lower(st.title),st.id",
    )
    .fetch_all(db)
    .await
    .map_err(|e| e.to_string())?;
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer
        .write_record([
            "artist",
            "title",
            "version",
            "preferred_source_url",
            "evidence_urls",
        ])
        .map_err(|e| e.to_string())?;
    for row in rows {
        let source_id: i64 = row.get(4);
        let urls: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT t.url FROM recommendations r
             JOIN recommendation_evidence e ON e.recommendation_id=r.id
             JOIN tracklists t ON t.id=e.tracklist_id
             WHERE r.source_track_id=?
               AND r.run_id=(SELECT id FROM discovery_runs WHERE import_id=(SELECT id FROM imports ORDER BY id DESC LIMIT 1) ORDER BY id DESC LIMIT 1)
             ORDER BY t.url",
        )
        .bind(source_id)
        .fetch_all(db)
        .await
        .map_err(|e| e.to_string())?;
        writer
            .write_record([
                row.get::<String, _>(0),
                row.get::<String, _>(1),
                row.get::<Option<String>, _>(2).unwrap_or_default(),
                row.get::<Option<String>, _>(3).unwrap_or_default(),
                urls.join(" | "),
            ])
            .map_err(|e| e.to_string())?;
    }
    String::from_utf8(writer.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_shortlist(db: tauri::State<'_, SqlitePool>) -> Result<String, String> {
    build_shortlist_csv(db.inner()).await
}

#[tauri::command]
async fn export_shortlist_to(path: String, db: tauri::State<'_, SqlitePool>) -> Result<(), String> {
    let destination = PathBuf::from(path);
    if destination.file_name().is_none() {
        return Err("Choose a CSV file destination".into());
    }
    let csv = build_shortlist_csv(db.inner()).await?;
    std::fs::write(&destination, csv)
        .map_err(|error| format!("Could not write {}: {error}", destination.display()))
}

fn normalize_identity(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn identity_key(
    artist: &str,
    title: &str,
    version: Option<&str>,
) -> (String, String, Option<String>) {
    (
        normalize_identity(artist),
        normalize_identity(title),
        version
            .map(normalize_identity)
            .filter(|value| !value.is_empty()),
    )
}

#[derive(Default)]
struct RankingSetEvidence {
    entry_count: i64,
    dj_name: Option<String>,
    seed_proximity: BTreeMap<i64, i64>,
}

#[derive(Default)]
struct RankingCandidate {
    artist: String,
    title: String,
    version: Option<String>,
    sets: BTreeMap<i64, RankingSetEvidence>,
}

async fn rank_run(db: &SqlitePool, run_id: i64) -> Result<(), String> {
    let import_id: i64 = sqlx::query_scalar("SELECT import_id FROM discovery_runs WHERE id=?")
        .bind(run_id)
        .fetch_optional(db)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Discovery run not found".to_string())?;
    let imported_rows =
        sqlx::query("SELECT artist,title,version FROM import_rows WHERE import_id=?")
            .bind(import_id)
            .fetch_all(db)
            .await
            .map_err(|error| error.to_string())?;
    let imported_identities: HashSet<_> = imported_rows
        .into_iter()
        .map(|row| {
            identity_key(
                row.get::<String, _>(0).as_str(),
                row.get::<String, _>(1).as_str(),
                row.get::<Option<String>, _>(2).as_deref(),
            )
        })
        .collect();
    let seed_ids: HashSet<i64> = sqlx::query_scalar(
        "SELECT sm.source_track_id FROM seed_matches sm
         JOIN import_rows ir ON ir.id=sm.import_row_id
         WHERE ir.import_id=? AND sm.status='accepted' AND sm.source_track_id IS NOT NULL",
    )
    .bind(import_id)
    .fetch_all(db)
    .await
    .map_err(|error| error.to_string())?
    .into_iter()
    .collect();
    let dismissed_ids: HashSet<i64> =
        sqlx::query_scalar("SELECT source_track_id FROM global_dismissals")
            .fetch_all(db)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .collect();

    let rows = sqlx::query(
        "SELECT candidate.source_track_id, source.artist, source.title, source.version,
                candidate.tracklist_id, candidate.position, seed.source_track_id, seed.position,
                tracklist.dj_name,
                (SELECT count(*) FROM tracklist_entries all_entries WHERE all_entries.tracklist_id=tracklist.id)
         FROM jobs tracklist_job
         JOIN tracklists tracklist ON tracklist.url=json_extract(tracklist_job.payload_json,'$.url')
         JOIN tracklist_entries candidate ON candidate.tracklist_id=tracklist.id
         JOIN source_tracks source ON source.id=candidate.source_track_id
         JOIN tracklist_entries seed ON seed.tracklist_id=tracklist.id
         JOIN seed_matches match ON match.source_track_id=seed.source_track_id AND match.status='accepted'
         JOIN import_rows seed_row ON seed_row.id=match.import_row_id
         WHERE tracklist_job.run_id=? AND tracklist_job.kind='fetch_tracklist'
           AND tracklist_job.status='completed' AND seed_row.import_id=?
           AND candidate.source_track_id IS NOT NULL
         ORDER BY candidate.source_track_id,candidate.tracklist_id,seed.source_track_id,candidate.position,seed.position",
    )
    .bind(run_id)
    .bind(import_id)
    .fetch_all(db)
    .await
    .map_err(|error| error.to_string())?;

    let mut candidates: BTreeMap<i64, RankingCandidate> = BTreeMap::new();
    for row in rows {
        let source_id: i64 = row.get(0);
        if seed_ids.contains(&source_id) || dismissed_ids.contains(&source_id) {
            continue;
        }
        let artist: String = row.get(1);
        let title: String = row.get(2);
        let version: Option<String> = row.get(3);
        if imported_identities.contains(&identity_key(&artist, &title, version.as_deref())) {
            continue;
        }
        let tracklist_id: i64 = row.get(4);
        let candidate_position: i64 = row.get(5);
        let seed_id: i64 = row.get(6);
        if source_id == seed_id {
            continue;
        }
        let seed_position: i64 = row.get(7);
        let candidate = candidates
            .entry(source_id)
            .or_insert_with(|| RankingCandidate {
                artist: artist.clone(),
                title: title.clone(),
                version: version.clone(),
                ..Default::default()
            });
        let evidence = candidate
            .sets
            .entry(tracklist_id)
            .or_insert_with(|| RankingSetEvidence {
                entry_count: row.get::<i64, _>(9).max(1),
                dj_name: row.get(8),
                ..Default::default()
            });
        let proximity = (candidate_position - seed_position).abs();
        evidence
            .seed_proximity
            .entry(seed_id)
            .and_modify(|current| *current = (*current).min(proximity))
            .or_insert(proximity);
    }

    let mut tx = db.begin().await.map_err(|error| error.to_string())?;
    sqlx::query("DELETE FROM recommendations WHERE run_id=?")
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    for (source_id, candidate) in candidates {
        let mut distinct_seeds = BTreeSet::new();
        let mut distinct_djs = BTreeSet::new();
        let mut seen_djs = HashMap::<String, usize>::new();
        let mut cooccurrence = 0.0;
        let mut proximity_score = 0.0;
        let mut adjacent_sets = 0_i64;
        for evidence in candidate.sets.values() {
            let dj_repeat_factor = if let Some(dj_name) = evidence.dj_name.as_deref() {
                let normalized = normalize_identity(dj_name);
                if normalized.is_empty() {
                    1.0
                } else {
                    distinct_djs.insert(normalized.clone());
                    let count = seen_djs.entry(normalized).or_default();
                    let factor = if *count == 0 { 1.0 } else { 0.65 };
                    *count += 1;
                    factor
                }
            } else {
                1.0
            };
            cooccurrence += dj_repeat_factor / (evidence.entry_count as f64).sqrt();
            let nearest = evidence
                .seed_proximity
                .values()
                .copied()
                .min()
                .unwrap_or(i64::MAX);
            if nearest <= 1 {
                adjacent_sets += 1;
            }
            if nearest != i64::MAX {
                proximity_score += 1.0 / (1.0 + nearest as f64);
            }
            distinct_seeds.extend(evidence.seed_proximity.keys().copied());
        }
        let set_count = candidate.sets.len() as i64;
        let seed_count = distinct_seeds.len() as i64;
        let dj_count = distinct_djs.len() as i64;
        let score = cooccurrence * 4.0
            + seed_count as f64 * 2.0
            + proximity_score * 2.0
            + (dj_count as f64).sqrt() * 0.5;
        let components = serde_json::json!({
            "setCount": set_count,
            "seedCount": seed_count,
            "djCount": dj_count,
            "adjacentCount": adjacent_sets,
            "cooccurrence": cooccurrence,
            "proximityScore": proximity_score,
            "formula": "cooccurrence*4 + seedCoverage*2 + proximity*2 + sqrt(djDiversity)*0.5",
            "candidateIdentity": {
                "artist": candidate.artist,
                "title": candidate.title,
                "version": candidate.version,
            }
        });
        let recommendation_id: i64 = sqlx::query(
            "INSERT INTO recommendations(run_id,source_track_id,score,components_json,ranking_version)
             VALUES(?,?,?,?,?) RETURNING id",
        )
        .bind(run_id)
        .bind(source_id)
        .bind(score)
        .bind(components.to_string())
        .bind(RANKING_VERSION)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| error.to_string())?
        .get(0);
        for (tracklist_id, evidence) in candidate.sets {
            for (seed_id, proximity) in evidence.seed_proximity {
                sqlx::query(
                    "INSERT INTO recommendation_evidence(recommendation_id,tracklist_id,seed_source_track_id,proximity)
                     VALUES(?,?,?,?)",
                )
                .bind(recommendation_id)
                .bind(tracklist_id)
                .bind(seed_id)
                .bind(proximity)
                .execute(&mut *tx)
                .await
                .map_err(|error| error.to_string())?;
            }
        }
    }
    tx.commit().await.map_err(|error| error.to_string())?;
    Ok(())
}

async fn load_cached_response(
    db: &SqlitePool,
    cache_key: &str,
    refresh_source: bool,
) -> Result<Option<serde_json::Value>, String> {
    if refresh_source {
        return Ok(None);
    }
    let response: Option<String> = sqlx::query_scalar(
        "SELECT response_json FROM page_cache
         WHERE cache_key=? AND adapter_version=?
           AND (expires_at IS NULL OR expires_at>CURRENT_TIMESTAMP)",
    )
    .bind(cache_key)
    .bind(DISCOVERY_ADAPTER_VERSION)
    .fetch_optional(db)
    .await
    .map_err(|error| error.to_string())?;
    response
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| format!("Cached source response is invalid: {error}"))
        })
        .transpose()
}

async fn run_status(db: &SqlitePool, run_id: i64) -> Result<String, String> {
    sqlx::query_scalar("SELECT status FROM discovery_runs WHERE id=?")
        .bind(run_id)
        .fetch_optional(db)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Discovery run not found".to_string())
}

async fn stop_if_requested(
    db: &SqlitePool,
    run_id: i64,
) -> Result<Option<DiscoveryRunSummary>, String> {
    if run_status(db, run_id).await? == "running" {
        return Ok(None);
    }
    discovery_run_summary(db, Some(run_id)).await
}

async fn wait_for_browser(
    db: &SqlitePool,
    run_id: i64,
    job_id: i64,
    error: &SourceAdapterFailure,
) -> Result<DiscoveryRunSummary, String> {
    if run_status(db, run_id).await? != "running" {
        return discovery_run_summary(db, Some(run_id))
            .await?
            .ok_or_else(|| "Discovery run not found".to_string());
    }
    let mut tx = db.begin().await.map_err(|value| value.to_string())?;
    sqlx::query(
        "UPDATE jobs SET status='queued',last_error=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(error.to_string())
    .bind(job_id)
    .execute(&mut *tx)
    .await
    .map_err(|value| value.to_string())?;
    sqlx::query(
        "UPDATE discovery_runs SET status='waiting_for_browser',message=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(format!(
        "{} Open the dedicated Chrome session, complete any normal interaction, then resume.",
        error.message
    ))
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .map_err(|value| value.to_string())?;
    tx.commit().await.map_err(|value| value.to_string())?;
    discovery_run_summary(db, Some(run_id))
        .await?
        .ok_or_else(|| "Discovery run not found".to_string())
}

fn canonical_tracklist_url(raw_url: &str) -> Option<String> {
    let mut url = tauri::Url::parse(raw_url).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("www.1001tracklists.com")
        || !url.path().starts_with("/tracklist/")
    {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

async fn schedule_tracklist_jobs(db: &SqlitePool, run_id: i64) -> Result<usize, String> {
    let appearance_jobs = sqlx::query(
        "SELECT payload_json,result_json FROM jobs
         WHERE run_id=? AND kind='fetch_appearances' AND status='completed'
         ORDER BY json_extract(payload_json,'$.seedOrder'),id",
    )
    .bind(run_id)
    .fetch_all(db)
    .await
    .map_err(|error| error.to_string())?;
    let mut by_seed = Vec::<(i64, Vec<String>)>::new();
    for row in appearance_jobs {
        let payload: serde_json::Value = serde_json::from_str(row.get::<String, _>(0).as_str())
            .map_err(|error| format!("Stored appearance job is invalid: {error}"))?;
        let Some(result_json) = row.get::<Option<String>, _>(1) else {
            continue;
        };
        let result: serde_json::Value = serde_json::from_str(&result_json)
            .map_err(|error| format!("Stored appearance result is invalid: {error}"))?;
        let seed_order = payload["seedOrder"].as_i64().unwrap_or(i64::MAX);
        let urls = result["appearances"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["url"].as_str())
            .filter_map(canonical_tracklist_url)
            .take(MAX_APPEARANCES_PER_SEED as usize)
            .collect();
        by_seed.push((seed_order, urls));
    }

    let mut selected = Vec::<(i64, usize, String)>::new();
    let mut seen = HashSet::new();
    let max_depth = by_seed
        .iter()
        .map(|(_, urls)| urls.len())
        .max()
        .unwrap_or(0);
    'rounds: for appearance_order in 0..max_depth {
        for (seed_order, urls) in &by_seed {
            if let Some(url) = urls.get(appearance_order) {
                if seen.insert(url.clone()) {
                    selected.push((*seed_order, appearance_order, url.clone()));
                    if selected.len() >= MAX_TRACKLISTS_PER_RUN as usize {
                        break 'rounds;
                    }
                }
            }
        }
    }

    let selected_urls: HashSet<_> = selected.iter().map(|(_, _, url)| url.clone()).collect();
    let stale_jobs = sqlx::query(
        "SELECT id,payload_json FROM jobs
         WHERE run_id=? AND kind='fetch_tracklist' AND status='queued'",
    )
    .bind(run_id)
    .fetch_all(db)
    .await
    .map_err(|error| error.to_string())?;
    let mut tx = db.begin().await.map_err(|error| error.to_string())?;
    for row in stale_jobs {
        let payload: serde_json::Value =
            serde_json::from_str(row.get::<String, _>(1).as_str()).unwrap_or_default();
        if payload["url"]
            .as_str()
            .is_none_or(|url| !selected_urls.contains(url))
        {
            sqlx::query("DELETE FROM jobs WHERE id=?")
                .bind(row.get::<i64, _>(0))
                .execute(&mut *tx)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    for (selection_order, (seed_order, appearance_order, url)) in selected.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO jobs(run_id,job_key,kind,status,payload_json,created_at,updated_at)
             VALUES(?,?,'fetch_tracklist','queued',?,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)
             ON CONFLICT(run_id,job_key) DO NOTHING",
        )
        .bind(run_id)
        .bind(format!("tracklist:{url}"))
        .bind(
            serde_json::json!({
                "url": url,
                "seedOrder": seed_order,
                "appearanceOrder": appearance_order,
                "selectionOrder": selection_order,
            })
            .to_string(),
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    }
    tx.commit().await.map_err(|error| error.to_string())?;
    Ok(selected_urls.len())
}

async fn store_appearance_result(
    db: &SqlitePool,
    job_id: i64,
    url: &str,
    result: &serde_json::Value,
) -> Result<(), String> {
    let mut tx = db.begin().await.map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO page_cache(cache_key,source_type,response_json,adapter_version,fetched_at,expires_at)
         VALUES(?,'appearances',?,?,CURRENT_TIMESTAMP,datetime('now','+1 day'))
         ON CONFLICT(cache_key) DO UPDATE SET response_json=excluded.response_json,
           adapter_version=excluded.adapter_version,fetched_at=CURRENT_TIMESTAMP,
           expires_at=datetime('now','+1 day')",
    )
    .bind(format!("appearances:{url}"))
    .bind(result.to_string())
    .bind(DISCOVERY_ADAPTER_VERSION)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE jobs SET status='completed',result_json=?,last_error=NULL,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(result.to_string())
    .bind(job_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())
}

async fn store_tracklist_result(
    db: &SqlitePool,
    job_id: i64,
    requested_url: &str,
    result: &serde_json::Value,
) -> Result<(), String> {
    let provider_id = result["providerId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Tracklist response omitted its provider ID".to_string())?;
    let entries = result["entries"]
        .as_array()
        .ok_or_else(|| "Tracklist response omitted its entries".to_string())?;
    let mut tx = db.begin().await.map_err(|error| error.to_string())?;
    let tracklist_id: i64 = sqlx::query(
        "INSERT INTO tracklists(provider,provider_id,url,title,dj_name,played_at,adapter_version,fetched_at)
         VALUES('1001tracklists',?,?,?,?,?,?,CURRENT_TIMESTAMP)
         ON CONFLICT(provider,provider_id) DO UPDATE SET url=excluded.url,title=excluded.title,
           dj_name=excluded.dj_name,played_at=excluded.played_at,
           adapter_version=excluded.adapter_version,fetched_at=CURRENT_TIMESTAMP RETURNING id",
    )
    .bind(provider_id)
    .bind(requested_url)
    .bind(result["title"].as_str())
    .bind(result["djName"].as_str())
    .bind(result["playedAt"].as_str())
    .bind(DISCOVERY_ADAPTER_VERSION)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?
    .get(0);
    sqlx::query("DELETE FROM tracklist_entries WHERE tracklist_id=?")
        .bind(tracklist_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    for item in entries {
        let display = item["displayText"].as_str().unwrap_or("Unknown").trim();
        let identity = display.split_once(" - ");
        let (artist, raw_title) = identity.unwrap_or(("Unknown", display));
        let (title, version) = parser::split_title_version(raw_title);
        let source_id = if let Some(provider_id) = item["providerId"].as_str() {
            Some(
                sqlx::query(
                    "INSERT INTO source_tracks(provider,provider_id,artist,title,version,adapter_version,url)
                     VALUES('1001tracklists',?,?,?,?,?,?)
                     ON CONFLICT(provider,provider_id) DO UPDATE SET artist=excluded.artist,
                       title=excluded.title,version=excluded.version,
                       adapter_version=excluded.adapter_version,url=excluded.url RETURNING id",
                )
                .bind(provider_id)
                .bind(artist.trim())
                .bind(title)
                .bind(version)
                .bind(DISCOVERY_ADAPTER_VERSION)
                .bind(item["trackUrl"].as_str())
                .fetch_one(&mut *tx)
                .await
                .map_err(|error| error.to_string())?
                .get::<i64, _>(0),
            )
        } else {
            None
        };
        sqlx::query(
            "INSERT INTO tracklist_entries(tracklist_id,position,source_track_id,display_text,cue_seconds)
             VALUES(?,?,?,?,?)",
        )
        .bind(tracklist_id)
        .bind(item["position"].as_i64().unwrap_or(0))
        .bind(source_id)
        .bind(display)
        .bind(item["cueSeconds"].as_i64())
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    }
    sqlx::query(
        "INSERT INTO page_cache(cache_key,source_type,response_json,adapter_version,fetched_at,expires_at)
         VALUES(?,'tracklist',?,?,CURRENT_TIMESTAMP,datetime('now','+30 days'))
         ON CONFLICT(cache_key) DO UPDATE SET response_json=excluded.response_json,
           adapter_version=excluded.adapter_version,fetched_at=CURRENT_TIMESTAMP,
           expires_at=datetime('now','+30 days')",
    )
    .bind(format!("tracklist:{requested_url}"))
    .bind(result.to_string())
    .bind(DISCOVERY_ADAPTER_VERSION)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE jobs SET status='completed',result_json=?,last_error=NULL,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(result.to_string())
    .bind(job_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn open_discovery_browser(
    app: tauri::AppHandle,
    run_id: i64,
    db: tauri::State<'_, SqlitePool>,
) -> Result<DiscoveryRunSummary, String> {
    let status = run_status(db.inner(), run_id).await?;
    if status != "waiting_for_browser" {
        return Err(format!(
            "The discovery browser can only be opened while a run is waiting for browser access, not while it is {status}"
        ));
    }
    let job = sqlx::query(
        "SELECT id,kind,payload_json FROM jobs
         WHERE run_id=? AND status='queued' AND
           (last_error LIKE 'BROWSER_CHALLENGE:%'
            OR last_error LIKE 'BROWSER_CHALLENGE_TIMEOUT:%'
            OR last_error LIKE 'BROWSER_SESSION_CLOSED:%')
         ORDER BY id LIMIT 1",
    )
    .bind(run_id)
    .fetch_optional(db.inner())
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "No browser-blocked discovery job was found".to_string())?;
    let job_id: i64 = job.get(0);
    let kind: String = job.get(1);
    let payload: serde_json::Value = serde_json::from_str(job.get::<String, _>(2).as_str())
        .map_err(|error| format!("Stored discovery job is invalid: {error}"))?;
    let (operation, url) = match kind.as_str() {
        "fetch_appearances" => (
            "fetchAppearances",
            payload["sourceUrl"]
                .as_str()
                .ok_or_else(|| "Appearance job omitted its source URL".to_string())?
                .to_string(),
        ),
        "fetch_tracklist" => (
            "fetchTracklist",
            payload["url"]
                .as_str()
                .ok_or_else(|| "Tracklist job omitted its URL".to_string())?
                .to_string(),
        ),
        _ => return Err("The blocked job cannot use browser handoff".into()),
    };

    let mut tx = db.begin().await.map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE jobs SET status='running',attempts=attempts+1,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(job_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE discovery_runs SET status='running',message='Chrome was opened for this blocked source page. Complete any normal interaction there.',updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())?;

    let request_payload = if kind == "fetch_appearances" {
        serde_json::json!({
            "url": &url,
            "interactive": true,
            "challengeTimeoutMs": 180_000,
            "limit": MAX_APPEARANCES_PER_SEED,
        })
    } else {
        serde_json::json!({
            "url": &url,
            "interactive": true,
            "challengeTimeoutMs": 180_000,
        })
    };
    match run_source_adapter(&app, operation, request_payload, 240_000, true).await {
        Ok(value) => {
            if kind == "fetch_appearances" {
                store_appearance_result(db.inner(), job_id, &url, &value).await?;
            } else {
                store_tracklist_result(db.inner(), job_id, &url, &value).await?;
            }
            sqlx::query(
                "UPDATE discovery_runs SET status='queued',message='Source access restored. Resume discovery; remaining requests will run headlessly.',updated_at=CURRENT_TIMESTAMP WHERE id=?",
            )
            .bind(run_id)
            .execute(db.inner())
            .await
            .map_err(|error| error.to_string())?;
        }
        Err(error) if error.needs_browser() => {
            return wait_for_browser(db.inner(), run_id, job_id, &error).await;
        }
        Err(error) => {
            let mut tx = db.begin().await.map_err(|value| value.to_string())?;
            sqlx::query(
                "UPDATE jobs SET status='failed',last_error=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
            )
            .bind(error.to_string())
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .map_err(|value| value.to_string())?;
            sqlx::query(
                "UPDATE discovery_runs SET status='queued',message=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
            )
            .bind(format!("The browser handoff failed: {error}"))
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|value| value.to_string())?;
            tx.commit().await.map_err(|value| value.to_string())?;
        }
    }
    discovery_run_summary(db.inner(), Some(run_id))
        .await?
        .ok_or_else(|| "Discovery run not found".to_string())
}

#[tauri::command]
async fn execute_discovery(
    app: tauri::AppHandle,
    run_id: i64,
    db: tauri::State<'_, SqlitePool>,
) -> Result<DiscoveryRunSummary, String> {
    let current = run_status(db.inner(), run_id).await?;
    if !matches!(
        current.as_str(),
        "queued" | "paused" | "waiting_for_browser"
    ) {
        return Err(format!(
            "Cannot execute a discovery run in {current} status"
        ));
    }
    let settings_json: String =
        sqlx::query_scalar("SELECT settings_json FROM discovery_runs WHERE id=?")
            .bind(run_id)
            .fetch_one(db.inner())
            .await
            .map_err(|error| error.to_string())?;
    let settings: serde_json::Value = serde_json::from_str(&settings_json)
        .map_err(|error| format!("Stored discovery settings are invalid: {error}"))?;
    let refresh_source = settings["refreshSource"].as_bool().unwrap_or(false);

    let mut tx = db.begin().await.map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE jobs SET status='queued',updated_at=CURRENT_TIMESTAMP
         WHERE run_id=? AND status='running'",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE discovery_runs SET status='running',stage='fetching_appearances',
         message='Fetching seed appearances one page at a time.',updated_at=CURRENT_TIMESTAMP
         WHERE id=?",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())?;

    let appearance_jobs = sqlx::query(
        "SELECT id,payload_json FROM jobs
         WHERE run_id=? AND kind='fetch_appearances' AND status='queued'
         ORDER BY json_extract(payload_json,'$.seedOrder'),id",
    )
    .bind(run_id)
    .fetch_all(db.inner())
    .await
    .map_err(|error| error.to_string())?;
    for job in appearance_jobs {
        if let Some(summary) = stop_if_requested(db.inner(), run_id).await? {
            return Ok(summary);
        }
        let job_id: i64 = job.get(0);
        let payload: serde_json::Value = serde_json::from_str(job.get::<String, _>(1).as_str())
            .map_err(|error| format!("Stored appearance job is invalid: {error}"))?;
        let url = payload["sourceUrl"]
            .as_str()
            .ok_or_else(|| "Appearance job omitted its source URL".to_string())?;
        sqlx::query(
            "UPDATE jobs SET status='running',attempts=attempts+1,updated_at=CURRENT_TIMESTAMP WHERE id=?",
        )
        .bind(job_id)
        .execute(db.inner())
        .await
        .map_err(|error| error.to_string())?;
        let cache_key = format!("appearances:{url}");
        let cached = load_cached_response(db.inner(), &cache_key, refresh_source).await?;
        let response = match cached {
            Some(value) => Ok(value),
            None => {
                run_source_adapter(
                    &app,
                    "fetchAppearances",
                    serde_json::json!({
                        "url": url,
                        "interactive": false,
                        "challengeTimeoutMs": 180_000,
                        "limit": MAX_APPEARANCES_PER_SEED,
                    }),
                    240_000,
                    false,
                )
                .await
            }
        };
        match response {
            Ok(value) => store_appearance_result(db.inner(), job_id, url, &value).await?,
            Err(error) if error.needs_browser() => {
                return wait_for_browser(db.inner(), run_id, job_id, &error).await;
            }
            Err(error) => {
                sqlx::query(
                    "UPDATE jobs SET status='failed',last_error=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
                )
                .bind(format!(
                    "{}{}",
                    error,
                    if error.retryable { " (retryable)" } else { "" }
                ))
                .bind(job_id)
                .execute(db.inner())
                .await
                .map_err(|value| value.to_string())?;
            }
        }
        if let Some(summary) = stop_if_requested(db.inner(), run_id).await? {
            return Ok(summary);
        }
    }

    let scheduled = schedule_tracklist_jobs(db.inner(), run_id).await?;
    sqlx::query(
        "UPDATE discovery_runs SET stage='fetching_tracklists',message=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(format!(
        "Fetching {scheduled} fairly selected unique tracklists."
    ))
    .bind(run_id)
    .execute(db.inner())
    .await
    .map_err(|error| error.to_string())?;
    let tracklist_jobs = sqlx::query(
        "SELECT id,payload_json FROM jobs
         WHERE run_id=? AND kind='fetch_tracklist' AND status='queued'
         ORDER BY json_extract(payload_json,'$.selectionOrder'),id",
    )
    .bind(run_id)
    .fetch_all(db.inner())
    .await
    .map_err(|error| error.to_string())?;
    for job in tracklist_jobs {
        if let Some(summary) = stop_if_requested(db.inner(), run_id).await? {
            return Ok(summary);
        }
        let job_id: i64 = job.get(0);
        let payload: serde_json::Value = serde_json::from_str(job.get::<String, _>(1).as_str())
            .map_err(|error| format!("Stored tracklist job is invalid: {error}"))?;
        let url = payload["url"]
            .as_str()
            .ok_or_else(|| "Tracklist job omitted its URL".to_string())?;
        sqlx::query(
            "UPDATE jobs SET status='running',attempts=attempts+1,updated_at=CURRENT_TIMESTAMP WHERE id=?",
        )
        .bind(job_id)
        .execute(db.inner())
        .await
        .map_err(|error| error.to_string())?;
        let cache_key = format!("tracklist:{url}");
        let cached = load_cached_response(db.inner(), &cache_key, refresh_source).await?;
        let response = match cached {
            Some(value) => Ok(value),
            None => {
                run_source_adapter(
                    &app,
                    "fetchTracklist",
                    serde_json::json!({
                        "url": url,
                        "interactive": false,
                        "challengeTimeoutMs": 180_000,
                    }),
                    240_000,
                    false,
                )
                .await
            }
        };
        match response {
            Ok(value) => store_tracklist_result(db.inner(), job_id, url, &value).await?,
            Err(error) if error.needs_browser() => {
                return wait_for_browser(db.inner(), run_id, job_id, &error).await;
            }
            Err(error) => {
                sqlx::query(
                    "UPDATE jobs SET status='failed',last_error=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
                )
                .bind(format!(
                    "{}{}",
                    error,
                    if error.retryable { " (retryable)" } else { "" }
                ))
                .bind(job_id)
                .execute(db.inner())
                .await
                .map_err(|value| value.to_string())?;
            }
        }
        if let Some(summary) = stop_if_requested(db.inner(), run_id).await? {
            return Ok(summary);
        }
    }

    sqlx::query(
        "UPDATE discovery_runs SET stage='ranking',message='Ranking candidates from persisted evidence.',updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(run_id)
    .execute(db.inner())
    .await
    .map_err(|error| error.to_string())?;
    rank_run(db.inner(), run_id).await?;
    let failures: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE run_id=? AND status='failed'")
            .bind(run_id)
            .fetch_one(db.inner())
            .await
            .map_err(|error| error.to_string())?;
    let final_status = if failures > 0 {
        "completed_with_errors"
    } else {
        "completed"
    };
    let message = if failures > 0 {
        format!(
            "Discovery completed with {failures} source error{}. Usable recommendations are ready.",
            if failures == 1 { "" } else { "s" }
        )
    } else {
        "Discovery complete. Recommendations are ready to audition.".into()
    };
    sqlx::query(
        "UPDATE discovery_runs SET status=?,message=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
    )
    .bind(final_status)
    .bind(message)
    .bind(run_id)
    .execute(db.inner())
    .await
    .map_err(|error| error.to_string())?;
    discovery_run_summary(db.inner(), Some(run_id))
        .await?
        .ok_or_else(|| "Discovery run not found".to_string())
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
            tauri::async_runtime::block_on(async {
                let mut tx = pool.begin().await?;
                sqlx::query(
                    "UPDATE jobs SET status='queued',updated_at=CURRENT_TIMESTAMP WHERE status='running'",
                )
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "UPDATE discovery_runs SET status='paused',message='The app closed during discovery. Resume to continue from persisted work.',updated_at=CURRENT_TIMESTAMP WHERE status='running'",
                )
                .execute(&mut *tx)
                .await?;
                tx.commit().await
            })?;
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
            control_discovery,
            list_recommendations,
            set_recommendation_feedback,
            attach_audio_source,
            mark_audio_source_wrong_version,
            export_shortlist,
            export_shortlist_to,
            open_discovery_browser,
            execute_discovery
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

            let run = create_discovery_run_record(&pool, false).await.unwrap();
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
            assert!(create_discovery_run_record(&pool, false)
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
            sqlx::query(
                "UPDATE jobs SET status='failed',last_error='BROWSER_CHALLENGE_TIMEOUT: waiting' WHERE id=(SELECT id FROM jobs WHERE run_id=? LIMIT 1)",
            )
            .bind(run.id)
            .execute(&pool)
            .await
            .unwrap();
            let paused_again = update_discovery_run_status(&pool, run.id, "pause")
                .await
                .unwrap();
            assert_eq!(paused_again.status, "paused");
            update_discovery_run_status(&pool, run.id, "resume")
                .await
                .unwrap();
            let retry_jobs: i64 =
                sqlx::query_scalar("SELECT count(*) FROM jobs WHERE run_id=? AND status='queued'")
                    .bind(run.id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(retry_jobs, 2);
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

            let replacement = create_discovery_run_record(&pool, false).await.unwrap();
            assert_ne!(replacement.id, run.id);
            assert_eq!(replacement.total_jobs, 2);
        });
    }

    #[test]
    fn ranks_golden_evidence_and_excludes_the_seed() {
        tauri::async_runtime::block_on(async {
            let (_directory, pool) = test_pool().await;
            let import_id: i64 = sqlx::query("INSERT INTO imports(name,source_path,encoding,delimiter) VALUES('Golden','/golden.txt','UTF-8','tab') RETURNING id").fetch_one(&pool).await.unwrap().get(0);
            add_accepted_seed(&pool, import_id, 1).await;
            let run = create_discovery_run_record(&pool, false).await.unwrap();
            let seed_id: i64 =
                sqlx::query_scalar("SELECT source_track_id FROM seed_matches LIMIT 1")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let candidate_id: i64 = sqlx::query("INSERT INTO source_tracks(provider,provider_id,artist,title,adapter_version,url) VALUES('1001tracklists','candidate','New Artist','New Track','fixture-v1','https://www.1001tracklists.com/track/candidate/new.html') RETURNING id").fetch_one(&pool).await.unwrap().get(0);
            let tracklist_id:i64=sqlx::query("INSERT INTO tracklists(provider,provider_id,url,title,dj_name,adapter_version) VALUES('1001tracklists','set-1','https://www.1001tracklists.com/tracklist/set-1/demo.html','Demo','DJ One','fixture-v1') RETURNING id").fetch_one(&pool).await.unwrap().get(0);
            sqlx::query("INSERT INTO jobs(run_id,job_key,kind,status,payload_json) VALUES(?,'tracklist:set-1','fetch_tracklist','completed','{\"url\":\"https://www.1001tracklists.com/tracklist/set-1/demo.html\"}')")
                .bind(run.id)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO tracklist_entries(tracklist_id,position,source_track_id,display_text) VALUES(?,1,?,'Seed'),(?,2,?,'New Artist - New Track')").bind(tracklist_id).bind(seed_id).bind(tracklist_id).bind(candidate_id).execute(&pool).await.unwrap();
            rank_run(&pool, run.id).await.unwrap();
            let ranked: Vec<i64> = sqlx::query_scalar(
                "SELECT source_track_id FROM recommendations WHERE run_id=? ORDER BY score DESC",
            )
            .bind(run.id)
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(ranked, vec![candidate_id]);
            let components: String =
                sqlx::query_scalar("SELECT components_json FROM recommendations WHERE run_id=?")
                    .bind(run.id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&components).unwrap()["adjacentCount"],
                1
            );
        });
    }

    #[test]
    fn schedules_unique_tracklists_fairly_across_seeds() {
        tauri::async_runtime::block_on(async {
            let (_directory, pool) = test_pool().await;
            let import_id: i64 = sqlx::query(
                "INSERT INTO imports(name,source_path,encoding,delimiter)
                 VALUES('Fair','/fair.txt','UTF-8','tab') RETURNING id",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
            add_accepted_seed(&pool, import_id, 1).await;
            add_accepted_seed(&pool, import_id, 2).await;
            let run = create_discovery_run_record(&pool, false).await.unwrap();
            let result_one = serde_json::json!({"appearances": [
                {"url":"https://www.1001tracklists.com/tracklist/a/one.html"},
                {"url":"https://www.1001tracklists.com/tracklist/b/two.html"}
            ]});
            let result_two = serde_json::json!({"appearances": [
                {"url":"https://www.1001tracklists.com/tracklist/c/three.html"},
                {"url":"https://www.1001tracklists.com/tracklist/a/one.html"},
                {"url":"https://www.1001tracklists.com/tracklist/d/four.html"}
            ]});
            sqlx::query(
                "UPDATE jobs SET status='completed',result_json=CASE
                   WHEN json_extract(payload_json,'$.seedOrder')=0 THEN ? ELSE ? END
                 WHERE run_id=? AND kind='fetch_appearances'",
            )
            .bind(result_one.to_string())
            .bind(result_two.to_string())
            .bind(run.id)
            .execute(&pool)
            .await
            .unwrap();

            assert_eq!(schedule_tracklist_jobs(&pool, run.id).await.unwrap(), 4);
            let urls: Vec<String> = sqlx::query_scalar(
                "SELECT json_extract(payload_json,'$.url') FROM jobs
                 WHERE run_id=? AND kind='fetch_tracklist'
                 ORDER BY json_extract(payload_json,'$.selectionOrder')",
            )
            .bind(run.id)
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(
                urls,
                vec![
                    "https://www.1001tracklists.com/tracklist/a/one.html",
                    "https://www.1001tracklists.com/tracklist/c/three.html",
                    "https://www.1001tracklists.com/tracklist/b/two.html",
                    "https://www.1001tracklists.com/tracklist/d/four.html",
                ]
            );
        });
    }
}
