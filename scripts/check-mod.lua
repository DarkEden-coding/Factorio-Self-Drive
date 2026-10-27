-- Offline checks for the data-stage companion and normalization. No engine emulation.
package.path = "mod/fsim-export/?.lua;" .. package.path
local normalize = require("normalize")
helpers = {table_to_json = function() return '{"obstacles":{},"resistances":{},"active_mods":{}}' end}
assert(normalize.json({}) == '{"obstacles":[],"resistances":[],"active_mods":{}}')
local routed
helpers.write_file = function(...) routed = table.pack(...) end
normalize.write_file("test", "[]", false, false)
assert(routed.n == 3, "Benchmark output must omit for_player")
normalize.write_file("test", "[]", false, nil)
assert(routed[4] == 0, "Console output must remain server-only")
normalize.write_file("test", "[]", false, 7)
assert(routed[4] == 7, "Player output must remain client-only")
assert(normalize.color({r = 1, g = 0.5})[2] == 128)
assert(normalize.color({r = 220, b = 10})[1] == 220)
assert(normalize.box({left_top = {x = -1, y = -2}, right_bottom = {x = 1, y = 2}})[1][2] == -2)
local catalog = require("catalog")
data = {raw = {car = {car = {}, tank = {immune_to_tree_impacts = true,
  immune_to_rock_impacts = true, immune_to_cliff_impacts = false, immune_to_all_impacts = true}}, cliff = {cliff = {
    name = "cliff", grid_size = {4, 4}, grid_offset = {0, 0.5}, orientations = {
      west_to_north = {collision_bounding_box = {{-2, -3}, {0, 1}, 0.125}}
    }
  }}}}
local companion
function data:extend(values) companion = values[1] end
dofile("mod/fsim-export/data-final-fixes.lua")
assert(companion.type == "mod-data")
assert(companion.data.vehicles.car.immune_to_cliff_impacts)
assert(not companion.data.vehicles.car.immune_to_tree_impacts)
assert(not companion.data.vehicles.car.immune_to_all_impacts)
assert(companion.data.vehicles.tank.immune_to_all_impacts)
assert(not companion.data.vehicles.tank.immune_to_cliff_impacts)
assert(companion.data.cliffs.orientations["west-to-north"].orientation == 0.125)
assert(companion.data.cliffs.grid_offset[2] == 0.5)
local names = {}
local scenarios = require("scenarios")
for _, s in ipairs(scenarios) do
  assert(not names[s.name], "duplicate scenario " .. s.name)
  names[s.name] = true
  assert(s.surface.area[1][1] == -s.surface.area[2][1])
  assert(s.surface.area[1][2] == -s.surface.area[2][2])
  for _, p in ipairs(s.surface.area) do assert(p[1] % 32 == 0 and p[2] % 32 == 0) end
  assert(s.vehicle.name == "car" or s.vehicle.name == "tank")
  assert(#s.inputs > 0)
  for _, input in ipairs(s.inputs) do
    assert(input.ticks > 0 and input.ticks % 1 == 0)
    assert(math.abs(input.throttle) <= 1 and math.abs(input.steer) <= 1)
  end
end
for _, vehicle in ipairs(catalog.vehicles) do
  for _, tile in ipairs({"grass-1", "dirt-1", "sand-1"}) do
    for _, fuel in ipairs(catalog.fuels) do
      assert(names[vehicle .. "-accelerate-" .. tile .. "-" .. fuel])
    end
  end
end
-- Exercise the runtime dump using ordinary tables in place of prototype handles.
prototypes = {entity = {}, item = {}, tile = {}, mod_data = {[companion.name] = {data = companion.data}}}
local function entity(name)
  return {name = name, type = "tree", collision_box = {{-0.4, -0.4}, {0.4, 0.4}},
    get_max_health = function() return 50 end, resistances = {impact = {decrease = 5, percent = 0.25}},
    consumption = 2500, braking_force = 3000, friction_force = 0.002,
    weight = 700, effectivity = 1, rotation_speed = 0.01, rotation_snap_angle = 0.01,
    terrain_friction_modifier = 1, energy_per_hit_point = 1, tank_driving = false,
    map_color = {r = 1, g = 0.5, b = 0}}
end
for _, name in ipairs(catalog.vehicles) do prototypes.entity[name] = entity(name) end
for _, name in ipairs(catalog.entities) do prototypes.entity[name] = entity(name) end
for _, name in ipairs(catalog.resources) do prototypes.entity[name] = entity(name) end
for _, name in ipairs(catalog.fuels) do prototypes.item[name] = {
  fuel_value = 4000000, fuel_acceleration_multiplier = 1, fuel_top_speed_multiplier = 1} end
for _, name in ipairs(catalog.tiles) do prototypes.tile[name] = {
  map_color = {r = 1, g = 0.5, b = 0}, vehicle_friction_modifier = 1.6,
  collision_mask = {layers = {ground_tile = true}}} end
script = {active_mods = {base = "2.1.23", ["fsim-export"] = "0.1.0"}}
local exported
helpers = {game_version = "2.1.23", table_to_json = function(value)
  exported = value
  return '{"cliffs":{"name":"cliff"}}'
end, write_file = function(path, json)
  assert(path == "fsim/prototypes.json")
  assert(json == '{"cliffs":{"max_health":null,"name":"cliff"}}')
end}
require("prototype-export").dump()
assert(exported.physics == nil)
assert(exported.schema_version == 2)
assert(not exported.vehicles[1].immune_to_all_impacts)
assert(exported.vehicles[2].immune_to_all_impacts)
assert(exported.source.game_version == "2.1.23")
assert(exported.vehicles[1].consumption == 150000)
assert(exported.vehicles[1].braking_power == 180000)
assert(exported.vehicles[1].resistances[1].percent == 25)
assert(#exported.tiles == 21 and #exported.resources == #catalog.resources)
print("Mod offline checks passed: companion, normalization, runtime dump, " .. #scenarios .. " scenarios")
