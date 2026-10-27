import type { MapPoolEntry } from "./types";

// Served from data/game/maps/ (see vite.config.ts).
export const MAP_POOL_URL = "/data/game/maps/";

export const POOL_HINT = "Generate the map pool with scripts/collect-game-data.sh";

export async function fetchPoolIndex(): Promise<MapPoolEntry[]> {
  const res = await fetch(`${MAP_POOL_URL}index.json`);
  if (!res.ok) throw new Error(`${MAP_POOL_URL}index.json: HTTP ${res.status}. ${POOL_HINT}`);
  const entries = (await res.json()) as MapPoolEntry[];
  if (!Array.isArray(entries) || entries.length === 0) throw new Error(`Map pool index is empty. ${POOL_HINT}`);
  return entries;
}

// Reads the whole body, reporting a 0..1 fraction when the server sends a length.
export async function fetchMapBytes(
  entry: MapPoolEntry,
  onProgress?: (fraction: number) => void,
): Promise<Uint8Array> {
  const url = `${MAP_POOL_URL}${entry.file}`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  const total = Number(res.headers.get("content-length"));
  if (!res.body || !total || !onProgress) return new Uint8Array(await res.arrayBuffer());

  const chunks: Uint8Array[] = [];
  let received = 0;
  const reader = res.body.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    onProgress(Math.min(1, received / total));
  }
  const bytes = new Uint8Array(received);
  let at = 0;
  for (const c of chunks) {
    bytes.set(c, at);
    at += c.length;
  }
  return bytes;
}
