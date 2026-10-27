import { copyFileSync, createReadStream, existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

const MAP_URL = "/data/game/maps/";
// The pool is generated outside app/ (scripts/collect-game-data.sh) so it is not duplicated in public/.
const MAP_DIR = fileURLToPath(new URL("../data/game/maps/", import.meta.url));
const MAP_TYPES: Record<string, string> = { ".json": "application/json", ".fsmap": "application/octet-stream" };

type Middleware = (req: { url?: string }, res: any, next: () => void) => void;

const serveMaps: Middleware = (req, res, next) => {
  const url = (req.url ?? "").split("?")[0];
  if (!url.startsWith(MAP_URL)) return next();
  // Flat directory: reject anything that is not a bare file name.
  const name = decodeURIComponent(url.slice(MAP_URL.length));
  const type = MAP_TYPES[name.slice(name.lastIndexOf("."))];
  const file = join(MAP_DIR, name);
  if (!type || name.includes("/") || name.includes("\\") || name.startsWith(".") || !existsSync(file)) {
    res.statusCode = 404;
    res.end("not found");
    return;
  }
  res.setHeader("Content-Type", type);
  res.setHeader("Content-Length", statSync(file).size);
  createReadStream(file).pipe(res);
};

// Serves data/game/maps/ at /data/game/maps/ in dev and preview, and copies it into the
// build output so a static host of dist/ works too.
function mapPool(): Plugin {
  let outDir = "dist";
  return {
    name: "fsim-map-pool",
    configResolved(config) {
      outDir = resolve(config.root, config.build.outDir);
    },
    configureServer(server) {
      server.middlewares.use(serveMaps);
    },
    configurePreviewServer(server) {
      server.middlewares.use(serveMaps);
    },
    closeBundle() {
      if (!existsSync(MAP_DIR)) {
        this.warn(`${MAP_DIR} not found; the build will have no map pool. Run scripts/collect-game-data.sh`);
        return;
      }
      const dest = join(outDir, MAP_URL);
      mkdirSync(dest, { recursive: true });
      for (const f of readdirSync(MAP_DIR)) {
        if (MAP_TYPES[f.slice(f.lastIndexOf("."))]) copyFileSync(join(MAP_DIR, f), join(dest, f));
      }
    },
  };
}

export default defineConfig({
  plugins: [mapPool()],
  server: {
    // Relative paths resolve against the app root. The wasm-pack output lives outside it.
    fs: { allow: [".", "../crates/fsim-wasm/pkg"] },
  },
});
