-- Prototype names in the simulator snapshot. Space Age planet names are added only when
-- that mod is active, so base-only dumps keep working.
local catalog = {
  vehicles = {"car", "tank"},
  fuels = {"wood", "coal", "solid-fuel", "rocket-fuel", "nuclear-fuel"},
  tiles = {"grass-1", "grass-2", "grass-3", "grass-4", "dry-dirt",
    "dirt-1", "dirt-2", "dirt-3", "dirt-4", "dirt-5", "dirt-6", "dirt-7",
    "red-desert-0", "red-desert-1", "red-desert-2", "red-desert-3",
    "sand-1", "sand-2", "sand-3", "water", "deepwater"},
  entities = {"big-rock", "big-sand-rock", "biter-spawner", "dead-dry-hairy-tree", "dead-grey-trunk", "dead-tree-desert", "dry-hairy-tree", "dry-tree", "huge-rock", "spitter-spawner", "tree-01", "tree-02", "tree-02-red", "tree-03", "tree-04", "tree-05", "tree-06", "tree-06-brown", "tree-07", "tree-08", "tree-08-brown", "tree-08-red", "tree-09", "tree-09-brown", "tree-09-red"},
  resources = {"iron-ore", "copper-ore", "coal", "stone", "uranium-ore", "crude-oil"},
  cliffs = {"cliff"}
}

-- Planets whose surfaces the collector exports, with their autoplaced tiles and the
-- natural obstacles found on them (2.1.24 Space Age data).
catalog.space_age = {
  planets = {"vulcanus", "gleba", "fulgora"},
  tiles = {
    "volcanic-soil-dark", "volcanic-soil-light", "volcanic-ash-soil", "volcanic-ash-flats",
    "volcanic-ash-light", "volcanic-ash-dark", "volcanic-cracks", "volcanic-cracks-warm",
    "volcanic-folds", "volcanic-folds-flat", "lava", "lava-hot", "volcanic-folds-warm",
    "volcanic-pumice-stones", "volcanic-cracks-hot", "volcanic-jagged-ground",
    "volcanic-smooth-stone", "volcanic-smooth-stone-warm", "volcanic-ash-cracks",
    "natural-yumako-soil", "natural-jellynut-soil", "wetland-yumako", "wetland-jellynut",
    "wetland-blue-slime", "wetland-light-green-slime", "wetland-green-slime",
    "wetland-light-dead-skin", "wetland-dead-skin", "wetland-pink-tentacle", "wetland-red-tentacle",
    "gleba-deep-lake", "lowland-brown-blubber", "lowland-olive-blubber", "lowland-olive-blubber-2",
    "lowland-olive-blubber-3", "lowland-pale-green", "lowland-cream-cauliflower",
    "lowland-cream-cauliflower-2", "lowland-dead-skin", "lowland-dead-skin-2", "lowland-cream-red",
    "lowland-red-vein", "lowland-red-vein-2", "lowland-red-vein-3", "lowland-red-vein-4",
    "lowland-red-vein-dead", "lowland-red-infection", "midland-turquoise-bark",
    "midland-turquoise-bark-2", "midland-cracked-lichen", "midland-cracked-lichen-dull",
    "midland-cracked-lichen-dark", "midland-yellow-crust", "midland-yellow-crust-2",
    "midland-yellow-crust-3", "midland-yellow-crust-4", "highland-dark-rock", "highland-dark-rock-2",
    "highland-yellow-rock", "pit-rock",
    "oil-ocean-shallow-2", "oil-ocean-shallow", "oil-ocean-deep", "oil-ocean-deep-2",
    "fulgoran-rock", "fulgoran-dust", "fulgoran-sand", "fulgoran-dunes", "fulgoran-walls",
    "fulgoran-paving", "fulgoran-conduit", "fulgoran-machinery"},
  entities = {
    "big-volcanic-rock", "huge-volcanic-rock", "big-volcanic-rock-hot", "huge-volcanic-rock-hot",
    "vulcanus-chimney", "vulcanus-chimney-faded", "vulcanus-chimney-cold", "vulcanus-chimney-short",
    "vulcanus-chimney-truncated", "ashland-lichen-tree", "ashland-lichen-tree-flaming",
    "water-cane", "funneltrunk", "boompuff", "stingfrond", "sunnycomb", "slipstack", "teflilly",
    "hairyclubnub", "cuttlepop", "lickmaw", "yumako-tree", "jellystem",
    "iron-stromatolite", "copper-stromatolite", "gleba-spawner", "gleba-spawner-small",
    "fulgoran-ruin-small", "fulgoran-ruin-medium", "fulgoran-ruin-stonehenge", "fulgoran-ruin-big",
    "fulgoran-ruin-huge", "fulgoran-ruin-colossal", "fulgoran-ruin-vault", "fulgoran-ruin-attractor",
    "fulgurite", "fulgurite-small", "fulgora-sunk-ruin-big", "fulgora-sunk-ruin-medium-tall",
    "big-fulgora-rock"},
  resources = {"calcite", "tungsten-ore", "sulfuric-acid-geyser", "scrap"},
  cliffs = {"cliff-vulcanus", "cliff-gleba", "cliff-fulgora", "crater-cliff"}
}

-- Returns the catalog for the active mods: base names, plus planet names with Space Age.
function catalog.active(space_age)
  local result = {}
  for _, key in ipairs({"vehicles", "fuels", "tiles", "entities", "resources", "cliffs"}) do
    result[key] = {}
    for _, name in ipairs(catalog[key]) do result[key][#result[key] + 1] = name end
    if space_age then
      for _, name in ipairs(catalog.space_age[key] or {}) do result[key][#result[key] + 1] = name end
    end
  end
  result.planets = space_age and catalog.space_age.planets or {}
  return result
end

return catalog
