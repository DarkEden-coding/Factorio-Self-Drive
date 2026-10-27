# Local Factorio graphics for the viewer

`fsim-autorun/data-final-fixes.lua` reads the game's resolved `data.raw` graphics
into a `mod-data` companion. Autorun writes `data/game/graphics.json` through the
collector. This captures all autoplace tiles, every tree and rock, both native
spawners, cliffs, resources, and car/tank definitions. The extraction uses the
installed base game; it does not scrape the website or infer atlas coordinates.

`scripts/collect-game-data.sh` calls the asset builder automatically. Run it alone
with `python3 scripts/build-game-assets.py`. Options are `--factorio <Contents>`,
`--input <graphics.json>` and `--output <directory>`. Defaults use this Mac's Steam
install, `data/game/graphics.json`, and `app/public/game-assets/`. The builder
resolves `__base__/` and `__core__/` paths against the installed `data/` directory,
copies only referenced PNGs, validates every rectangle against PNG dimensions,
removes stale generated PNGs, and prints the total copied bytes. It needs only
Python's standard library.

These PNGs are proprietary game assets for the user's local viewer. The entire
`app/public/game-assets/` directory, including the manifest, is gitignored. Do
not commit or redistribute it. `data/game/graphics.json` is also gitignored.
It records definitions and paths but contains no image pixels. Regenerate both
from the user's own Factorio installation.

## Manifest version 1

Fetch `/game-assets/manifest.json`. Top-level keys are `schema_version: 1`,
`tile_pixels: 32`, `tiles`, `entities`, `cliffs`, `resources`, `vehicles`,
`images`, and `summary`. Object keys are prototype names unless specified below.
Every sprite is already an explicit rectangle, with no sprite-sheet parsing
required in the frontend:

```ts
type Sprite = {
  image: string;             // relative to /game-assets/, e.g. base/graphics/...
  x: number; y: number;       // source pixels, top-left
  width: number; height: number;
  scale: number;             // source pixels -> game pixels at 32 pixels/tile
  shift: [number, number];   // tiles, east/south; NOT pixels
  draw_as_shadow: boolean;
  tint?: [number, number, number, number]; // normalized RGBA multiplier
};
type Variations = { variations: Sprite[][] };
```

Draw a sprite centered at its anchor plus `shift`. At zoom P pixels per tile,
its destination width is `width * scale * P / 32` and its height follows the
same rule. Destination center is `[(worldX + shift[0])*P,
(worldY + shift[1])*P]` before applying the camera transform. Tint multiplies
source RGBA channels; it is not a solid color overlay. Shadow sprites are
the game's shadow masks. Drawn opaque they look black, so the viewer draws the
shadow pass at 50% opacity to approximate the game's shadow compositing. Layer arrays are in paint order. A renderer may collect shadows into
a global shadow pass and sort non-shadow entity layers by entity y.

`tiles[name]` is `{ size_tiles: 1, variations: Sprite[][] }`. Each variation has
one sprite from that tile's 1×1 `variants.main` arrangement. Its anchor is the
tile center `(tx + 0.5, ty + 0.5)`. Water uses the game's static water image.
Terrain is opaque and drawn first. These are actual game textures, with one
independent texture variation per tile. The manifest intentionally omits game
transition masks, water animation, decorative entities, lights and reflections.
It does not reproduce the game's shoreline blending or tile patch weighting.
The current 2.1 Nauvis tiles all supply `variants.main`; no material-background
fallback is needed for this installed data set.

`entities[name]` is `Variations`. Trees contain shadow, trunk, then healthy leaf
layers. Leaf colors come from the tree prototype color list. Dead trees and rocks
use `pictures`; nests use the first frame of each idle animation variation.
Normal maps, damaged leaf frames and stumps are omitted. Entity positions are the
sprite anchors. Use the original `entity.name`, even when a map import reported
a physics fallback. Unknown user-modded names have no guaranteed manifest entry;
fall back by `entity.kind` or show the palette debug rendering.

`cliffs[orientation]` is `Variations`, keyed by Factorio hyphenated orientation,
for example `west-to-north`. Use `entity.cliff_orientation`. The graphics already
encode their orientation, so do not rotate them using the collision-box
`entity.orientation`. Main cliff pictures include shadow layers. The alternate
lower pictures for platform-style rendering are not used on Nauvis.

`resources[name]` is `{ stage: 0, variations: Sprite[][] }`. It has one static
richest-stage frame per shape variation, including crude oil. Each resource cell
is anchored at its tile center. The simulator resource palette maps cell IDs to
names; 0 means no resource. Ore amounts and depletion stages are not simulated.

`vehicles.car` and `vehicles.tank` are
`{ direction_count: 64, directions: Sprite[][] }`. Each direction includes body,
runtime color-mask, turret, and shadow layers. The turret points with the body;
there is no independent turret state in the simulator. Runtime masks have a
fixed orange tint `[0.9,0.5,0.2,1]`. The builder expands `stripes`, `filenames`,
`lines_per_file`, `line_length`, and repeated animation-frame stripes into
explicit static frame-zero rectangles. Body and turret shadows precede their
visible layers. Do not rotate the destination rectangle a second time.

Factorio orientation is clockwise, 0 north, 0.25 east. Select the nearest vehicle
direction using `Math.floor((((orientation % 1) + 1) % 1) * direction_count + 0.5)
% direction_count`. Direction 0 is north. The mask and shadow stripe boundaries
are included in the builder regression tests.

## Deterministic choices

Use `(entity.id - 1) % variations.length` for entities and cliffs. For tiles and
resources use this unsigned 32-bit hash of *source* tile coordinates:

```ts
const tileHash = (x: number, y: number) =>
  (Math.imul(x | 0, 73856093) ^ Math.imul(y | 0, 19349663)) >>> 0;
const variation = tileHash(sourceX, sourceY) % variations.length;
```

Source tile coordinates equal simulator tile coordinates plus
`map_info().source_offset`. This keeps tile appearance stable if an export is
recentered. Entity IDs are deterministic for the same map file. Choices are
viewer conventions, not the engine's hidden visual random state.

`images[image]` is `{ width, height, bytes }` for each copied PNG. Every listed
image is referenced by at least one sprite. `summary` contains `image_count`,
`sprite_count` and `image_bytes`. The manifest itself is not included in
`image_bytes`. Load images on demand or preload names in the current view.
