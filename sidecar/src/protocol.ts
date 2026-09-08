import { z } from "zod";

export const PROTOCOL_VERSION = 1;
export const Request = z.object({
  version: z.literal(PROTOCOL_VERSION),
  requestId: z.string().min(1).max(128),
  operation: z.enum(["searchTracks", "fetchTrack", "fetchAppearances", "fetchTracklist"]),
  payload: z.record(z.string(), z.unknown()),
  timeoutMs: z.number().int().min(1_000).max(120_000).default(30_000),
});
export type Request = z.infer<typeof Request>;
export type Response = { version: 1; requestId: string; ok: true; result: unknown } | { version: 1; requestId: string; ok: false; error: { code: string; message: string; retryable: boolean } };

export const SearchTracksPayload = z.object({
  artist: z.string().trim().min(1).max(300),
  title: z.string().trim().min(1).max(500),
  version: z.string().trim().max(300).nullish().transform(value => value ?? undefined),
  limit: z.number().int().min(1).max(25).default(10),
});

export const FetchTrackPayload = z.object({
  url: z.string().url(),
});

export function errorResponse(requestId: string, code: string, message: string, retryable = false): Response {
  return { version: PROTOCOL_VERSION, requestId, ok: false, error: { code, message, retryable } };
}

export function successResponse(requestId: string, result: unknown): Response {
  return { version: PROTOCOL_VERSION, requestId, ok: true, result };
}
