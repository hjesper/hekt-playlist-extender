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

export type DiscoveryRun = {
  id: number;
  status: "queued" | "running" | "waiting_for_review" | "waiting_for_browser" | "paused" | "completed" | "completed_with_errors" | "failed" | "cancelled";
  stage: "matching" | "fetching_appearances" | "fetching_tracklists" | "ranking";
  message?: string;
  queuedJobs: number;
  runningJobs: number;
  completedJobs: number;
  failedJobs: number;
  totalJobs: number;
  maxAppearancesPerSeed: number;
  maxTracklists: number;
  createdAt: string;
  updatedAt: string;
};
