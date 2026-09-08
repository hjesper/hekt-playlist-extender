mod parser;

use parser::{parse_playlist, ImportPreview};
use serde::Serialize;
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::{path::PathBuf, str::FromStr};
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceSearchInput {
    artist: String,
    title: String,
    version: Option<String>,
    limit: Option<u8>,
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
    let request = serde_json::json!({
        "version": 1,
        "requestId": format!("ui-{}", std::process::id()),
        "operation": "searchTracks",
        "payload": { "artist": artist, "title": title, "version": input.version, "limit": limit },
        "timeoutMs": 45_000
    });
    let output = app
        .shell()
        .sidecar("hekt-source-adapter")
        .map_err(|error| format!("Could not prepare source adapter: {error}"))?
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
        let message = response
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Source search failed");
        return Err(message.to_string());
    }
    Ok(response
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
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
            search_source
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hekt");
}
