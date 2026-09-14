import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type { DiscoveryRun, ImportPreview, LibrarySummary, Recommendation, SourceSearchResult, SourceTrackDetail, Track } from "./types";

const isTauri = () => "__TAURI_INTERNALS__" in window;

const demoTracks: Track[] = [
  { id: 1, rowNumber: 1, artist: "Bicep", title: "Glue", version: "Original Mix", label: "Ninja Tune", bpm: 129, key: "A min", selected: true, matchStatus: "accepted" },
  { id: 2, rowNumber: 2, artist: "Overmono", title: "So U Kno", label: "XL Recordings", bpm: 130, key: "F min", selected: true, matchStatus: "accepted" },
  { id: 3, rowNumber: 3, artist: "DJ Seinfeld", title: "U", version: "Extended", label: "Ninja Tune", bpm: 128, selected: false },
  { id: 4, rowNumber: 4, artist: "Logic1000", title: "What You Like", label: "Because Music", bpm: 126, selected: false },
];

const demoDiscoveryRun: DiscoveryRun = {
  id: 1,
  status: "queued",
  stage: "fetching_appearances",
  message: "Queue prepared. Valid cached pages will be reused.",
  queuedJobs: 2,
  runningJobs: 0,
  completedJobs: 0,
  failedJobs: 0,
  totalJobs: 2,
  maxAppearancesPerSeed: 25,
  maxTracklists: 100,
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

export async function getSummary(): Promise<LibrarySummary> {
  return isTauri() ? invoke("library_summary") : { imports: 1, tracks: 4, selectedSeeds: 2, pendingSeeds: 0, acceptedSeeds: 2, skippedSeeds: 0, importName: "Friday warm-up" };
}

export async function loadTracks(): Promise<Track[]> {
  return isTauri() ? invoke("list_tracks") : demoTracks;
}

export async function previewPlaylist(path: string): Promise<ImportPreview> {
  return invoke("preview_playlist", { path });
}

export async function importPlaylist(path: string, name: string): Promise<ImportPreview> {
  return invoke("import_playlist", { path, name });
}

export async function setSeed(trackId: number, selected: boolean): Promise<void> {
  if (isTauri()) await invoke("set_seed", { trackId, selected });
}

export async function resolveSeed(trackId: number, status: "pending" | "accepted" | "skipped", sourceUrl?: string): Promise<void> {
  if (isTauri()) await invoke("resolve_seed", { trackId, status, sourceUrl });
}

export async function searchSource(track: Track, broad = false): Promise<SourceSearchResult> {
  if (!isTauri()) return { query: `${track.artist} ${track.title}`, resultUrl: "", tracks: [] };
  return invoke("search_source", { input: { artist: track.artist, title: track.title, version: broad ? undefined : track.version, limit: 10 } });
}

export async function verifySourceTrack(url: string): Promise<SourceTrackDetail> {
  if (!isTauri()) return { url, title: "Demo source track", appearances: [] };
  return invoke("verify_source_track", { url });
}

export async function getLatestDiscoveryRun(): Promise<DiscoveryRun | null> {
  return isTauri() ? invoke("latest_discovery_run") : demoDiscoveryRun;
}

export async function startDiscovery(refreshSource = false): Promise<DiscoveryRun> {
  return isTauri() ? invoke("start_discovery", { refreshSource }) : demoDiscoveryRun;
}

export async function controlDiscovery(runId: number, action: "pause" | "resume" | "cancel"): Promise<DiscoveryRun> {
  if (isTauri()) return invoke("control_discovery", { runId, action });
  const status = action === "pause" ? "paused" : action === "resume" ? "queued" : "cancelled";
  return {...demoDiscoveryRun, status};
}

export async function executeDiscovery(runId: number): Promise<DiscoveryRun> {
  return isTauri()
    ? invoke("execute_discovery", { runId })
    : {...demoDiscoveryRun, status: "completed", stage: "ranking", message: "Discovery complete."};
}

export async function openDiscoveryBrowser(runId: number): Promise<DiscoveryRun> {
  return isTauri()
    ? invoke("open_discovery_browser", {runId})
    : {...demoDiscoveryRun, status: "queued", message: "Source access restored. Resume discovery headlessly."};
}

const demoRecommendations: Recommendation[] = [
  {id: 1, sourceTrackId: 101, artist: "Four Tet", title: "Baby", score: 12.75, setCount: 7, seedCount: 2, djCount: 4, adjacentCount: 2, sourceUrl: "https://www.youtube.com/watch?v=i1gVxKhdGPs", sourceProvider: "youtube", playbackStatus: "available", evidenceUrls: ["https://www.1001tracklists.com/tracklist/demo/four-tet.html"]},
  {id: 2, sourceTrackId: 102, artist: "Floating Points", title: "Bias", version: "Mayfield Depot Mix", score: 9.5, setCount: 5, seedCount: 2, djCount: 3, adjacentCount: 1, evidenceUrls: ["https://www.1001tracklists.com/tracklist/demo/floating-points.html"]}
];

export async function listRecommendations(): Promise<Recommendation[]> {
  return isTauri() ? invoke("list_recommendations") : demoRecommendations;
}

export async function setRecommendationFeedback(sourceTrackId: number, disposition?: "saved" | "rejected" | "dismissed"): Promise<void> {
  if (isTauri()) await invoke("set_recommendation_feedback", { sourceTrackId, disposition });
}

export async function attachAudioSource(sourceTrackId: number, provider: string, url: string): Promise<void> {
  if (isTauri()) await invoke("attach_audio_source", { sourceTrackId, provider, url });
}

export async function markAudioSourceWrongVersion(sourceTrackId: number): Promise<void> {
  if (isTauri()) await invoke("mark_audio_source_wrong_version", { sourceTrackId });
}

export async function exportShortlist(): Promise<void> {
  if (isTauri()) {
    const path = await save({
      defaultPath: "hekt-shortlist.csv",
      filters: [{name: "CSV shortlist", extensions: ["csv"]}],
    });
    if (path) await invoke("export_shortlist_to", {path});
    return;
  }
  const csv = "artist,title,version,preferred_source_url,evidence_urls\nFour Tet,Baby,,https://youtube.com,https://1001tracklists.com\n";
  const blob = new Blob([csv], {type: "text/csv;charset=utf-8"});
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = "hekt-shortlist.csv";
  anchor.click();
  URL.revokeObjectURL(url);
}
