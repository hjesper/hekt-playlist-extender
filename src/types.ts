export type Track = {
  id: number;
  rowNumber: number;
  artist: string;
  title: string;
  version?: string;
  label?: string;
  bpm?: number;
  key?: string;
  duration?: string;
  selected: boolean;
  matchStatus?: "pending" | "accepted" | "skipped";
  sourceUrl?: string;
  originalFields?: Record<string, string>;
};

export type ImportPreview = {
  name: string;
  encoding: string;
  delimiter: string;
  headers: string[];
  tracks: Track[];
  warnings: string[];
};

export type LibrarySummary = {
  imports: number;
  tracks: number;
  selectedSeeds: number;
  pendingSeeds: number;
  acceptedSeeds: number;
  skippedSeeds: number;
  importName?: string;
};

export type SourceTrackResult = {
  providerId: string;
  url: string;
  displayText: string;
};

export type SourceSearchResult = {
  query: string;
  resultUrl: string;
  tracks: SourceTrackResult[];
};

export type SourceTrackDetail = {
  url: string;
  title: string;
  appearances: Array<{url: string; label: string}>;
};
