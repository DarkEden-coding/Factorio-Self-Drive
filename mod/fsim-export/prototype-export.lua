local catalog_module = require("catalog")
local normalize = require("normalize")
local M = {}
local function resistances(p)
  local result = {}
  for name, r in pairs(p.resistances or {}) do
    result[#result + 1] = {type = name, decrease = r.decrease or 0, percent = (r.percent or 0) * 100}
  end
  table.sort(result, function(a, b) return a.type < b.type end)
  return result
end
function M.dump(player)
  local companion = prototypes.mod_data["fsim-export-companion"].data
  local catalog = catalog_module.active(script.active_mods["space-age"] ~= nil)
  local result = {schema_version = 2, source = normalize.source(), vehicles = {}, fuels = {},
    tiles = {}, entities = {}, resources = {}, cliffs = companion.cliffs,
    other_cliffs = companion.other_cliffs or {}}
  for _, name in ipairs(catalog.vehicles) do
    local p = assert(prototypes.entity[name], "Missing vehicle " .. name)
    local v = {name = name, consumption = p.consumption * 60, braking_power = p.braking_force * 60,
      friction = p.friction_force, max_health = p.get_max_health("normal"),
      collision_box = normalize.box(p.collision_box), resistances = resistances(p)}
    for _, key in ipairs({"weight", "effectivity", "rotation_speed", "rotation_snap_angle",
      "terrain_friction_modifier", "energy_per_hit_point", "tank_driving"}) do v[key] = p[key] end
    for key, value in pairs(companion.vehicles[name]) do v[key] = value end
    result.vehicles[#result.vehicles + 1] = v
  end
  for _, name in ipairs(catalog.fuels) do
    local p = assert(prototypes.item[name], "Missing fuel " .. name)
    result.fuels[#result.fuels + 1] = {name = name, fuel_value = p.fuel_value,
      fuel_acceleration_multiplier = p.fuel_acceleration_multiplier,
      fuel_top_speed_multiplier = p.fuel_top_speed_multiplier}
  end
  for i, name in ipairs(catalog.tiles) do
    local p = assert(prototypes.tile[name], "Missing tile " .. name)
    local layers = {}
    for layer, enabled in pairs(p.collision_mask.layers) do
      if enabled then layers[#layers + 1] = layer end
    end
    table.sort(layers)
    result.tiles[#result.tiles + 1] = {id = i - 1, name = name, color = normalize.color(p.map_color),
      vehicle_friction_modifier = p.vehicle_friction_modifier, collision_mask = layers,
      -- "water" in the simulator means any tile a vehicle can't enter (also lava, deep lakes).
      is_water = normalize.blocks_vehicles(p.collision_mask)}
  end
  for _, name in ipairs(catalog.entities) do
    local p = assert(prototypes.entity[name], "Missing obstacle " .. name)
    result.entities[#result.entities + 1] = {name = name, kind = normalize.kind(p.type),
      max_health = p.get_max_health("normal"), collision_box = normalize.box(p.collision_box),
      resistances = resistances(p)}
  end
  for i, name in ipairs(catalog.resources) do
    local p = assert(prototypes.entity[name], "Missing resource " .. name)
    result.resources[#result.resources + 1] = {id = i, name = name, color = normalize.color(p.map_color)}
  end
  -- Lua has no JSON-null value. Insert the schema's null explicitly after serialization.
  local json = normalize.json(result)
  json = json:gsub('"cliffs"%s*:%s*{', '"cliffs":{"max_health":null,', 1)
  normalize.write_file("fsim/prototypes.json", json, false, player)
end
return M
