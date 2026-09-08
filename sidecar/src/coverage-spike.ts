import { readFile } from "node:fs/promises";
import { SourceAdapter, type TrackSearchResult } from "./adapter.js";
import { PROTOCOL_VERSION } from "./protocol.js";

type SampleTrack = { row: number; artist: string; title: string; version?: string };

function parseVersion(rawTitle: string): {title:string;version?:string} {
  const match = rawTitle.match(/^(.*?)\s*\(([^()]*(?:mix|remix|edit|dub|version|rework|bootleg|radio|extended)[^()]*)\)$/i);
  return match ? { title: match[1].trim(), version: match[2].trim() } : { title: rawTitle };
}

function normalize(value: string): string {
  return value.normalize("NFKD").replace(/\p{M}/gu, "").toLowerCase().replace(/&/g, " and ").replace(/[^a-z0-9]+/g, " ").trim();
}

function classify(track: SampleTrack, results: TrackSearchResult[]): "strong"|"ambiguous"|"missing" {
  const artist = normalize(track.artist);
  const title = normalize(track.title);
  const version = track.version && normalize(track.version);
  const candidates = results.filter(result => {
    const display = normalize(result.displayText);
    return display.includes(artist) && display.includes(title);
  });
  if (!candidates.length) return "missing";
  if (!version) {
    const exact = normalize(`${track.artist} - ${track.title}`);
    return candidates.some(result => normalize(result.displayText) === exact) ? "strong" : "ambiguous";
  }
  return candidates.some(result => normalize(result.displayText).includes(version)) ? "strong" : "ambiguous";
}

async function readSample(path: string): Promise<SampleTrack[]> {
  const bytes = await readFile(path);
  const encoding = bytes[0] === 0xff && bytes[1] === 0xfe ? "utf-16le" : "utf-8";
  const text = new TextDecoder(encoding).decode(encoding === "utf-16le" ? bytes.subarray(2) : bytes);
  const rows = text.split(/\r?\n/).filter(Boolean).map(line => line.split("\t"));
  const headers = rows.shift()?.map(header => header.replace(/^\uFEFF/, "")) ?? [];
  const artistIndex = headers.indexOf("Artist");
  const titleIndex = headers.indexOf("Track Title");
  if (artistIndex < 0 || titleIndex < 0) throw new Error("Sample needs Artist and Track Title columns");
  return rows.flatMap((row, index) => {
    const artist = row[artistIndex]?.trim();
    const rawTitle = row[titleIndex]?.trim();
    if (!artist || !rawTitle) return [];
    return [{ row: index + 1, artist, ...parseVersion(rawTitle) }];
  });
}

const path = process.argv[2];
const requestedCount = Number(process.argv[3] ?? 20);
if (!path) throw new Error("Usage: coverage-spike <playlist.txt> [sample-count]");
if (!Number.isInteger(requestedCount) || requestedCount < 1 || requestedCount > 20) throw new Error("Sample count must be 1–20");

const tracks = await readSample(path);
const selected = Array.from({length: Math.min(requestedCount, tracks.length)}, (_, index) =>
  tracks[Math.round(index * (tracks.length - 1) / Math.max(1, requestedCount - 1))]
);
const adapter = new SourceAdapter();
const findings: Array<SampleTrack & {classification:string;results:TrackSearchResult[]}> = [];
try {
  for (const [index, track] of selected.entries()) {
    const response = await adapter.execute({
      version: PROTOCOL_VERSION,
      requestId: `coverage-${index + 1}`,
      operation: "searchTracks",
      payload: {artist:track.artist,title:track.title,version:track.version,limit:10},
      timeoutMs: 45_000,
    }) as {tracks:TrackSearchResult[]};
    findings.push({...track,classification:classify(track,response.tracks),results:response.tracks.slice(0,3)});
    await new Promise(resolve => setTimeout(resolve, 750));
  }
} finally {
  await adapter.close();
}
const summary = findings.reduce((counts, finding) => ({...counts,[finding.classification]:(counts[finding.classification] ?? 0) + 1}), {} as Record<string,number>);
process.stdout.write(`${JSON.stringify({source:path,totalEligible:tracks.length,sampleSize:findings.length,summary,findings},null,2)}\n`);
