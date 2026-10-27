local catalog = require("catalog")
local normalize = require("normalize")
local companion = {vehicles = {}, cliffs = {orientations = {}}}
for _, name in ipairs(catalog.vehicles) do
  local p = assert(data.raw.car[name], "Missing vehicle " .. name)
  companion.vehicles[name] = {
    immune_to_tree_impacts = p.immune_to_tree_impacts == true,
    immune_to_rock_impacts = p.immune_to_rock_impacts == true,
    immune_to_cliff_impacts = p.immune_to_cliff_impacts ~= false,
    immune_to_all_impacts = p.immune_to_all_impacts == true
  }
end
local p = assert(data.raw.cliff.cliff, "Missing cliff prototype")
companion.cliffs.name = p.name
companion.cliffs.grid_size = normalize.position(p.grid_size)
companion.cliffs.grid_offset = normalize.position(p.grid_offset)
for name, orientation in pairs(p.orientations) do
  local b = orientation.collision_bounding_box
  companion.cliffs.orientations[name:gsub("_", "-")] = {
    collision_box = normalize.box(b), orientation = b.orientation or b[3] or 0
  }
end
data:extend({{type = "mod-data", name = "fsim-export-companion", data_type = "fsim.prototype-companion.v2",
  data = companion}})
