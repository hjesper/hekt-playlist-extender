import { invoke } from "@tauri-apps/api/core";
import type { ImportPreview, LibrarySummary, SourceSearchResult, SourceTrackDetail, Track } from "./types";

const isTauri = () => "__TAURI_INTERNALS__" in window;

const demoTracks: Track[] = [
  { id: 1, rowNumber: 1, artist: "Bicep", title: "Glue", version: "Original Mix", label: "Ninja Tune", bpm: 129, key: "A min", selected: true },
  { id: 2, rowNumber: 2, artist: "Overmono", title: "So U Kno", label: "XL Recordings", bpm: 130, key: "F min", selected: true },
  { id: 3, rowNumber: 3, artist: "DJ Seinfeld", title: "U", version: "Extended", label: "Ninja Tune", bpm: 128, selected: false },
  { id: 4, rowNumber: 4, artist: "Logic1000", title: "What You Like", label: "Because Music", bpm: 126, selected: false },
];

export async function getSummary(): Promise<LibrarySummary> {
  return isTauri() ? invoke("library_summary") : { imports: 1, tracks: 4, selectedSeeds: 2, pendingSeeds: 1, acceptedSeeds: 1, skippedSeeds: 0, importName: "Friday warm-up" };
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
