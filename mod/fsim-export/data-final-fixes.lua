local catalog = require("catalog")
local normalize = require("normalize")
local companion = {vehicles = {}}
for _, name in ipairs(catalog.vehicles) do
  local p = assert(data.raw.car[name], "Missing vehicle " .. name)
  companion.vehicles[name] = {
    immune_to_tree_impacts = p.immune_to_tree_impacts == true,
    immune_to_rock_impacts = p.immune_to_rock_impacts == true,
    immune_to_cliff_impacts = p.immune_to_cliff_impacts ~= false,
    immune_to_all_impacts = p.immune_to_all_impacts == true
  }
end
local function cliff(p)
  local result = {name = p.name, grid_size = normalize.position(p.grid_size),
    grid_offset = normalize.position(p.grid_offset), orientations = {}}
  for name, orientation in pairs(p.orientations) do
    local b = orientation.collision_bounding_box
    result.orientations[name:gsub("_", "-")] = {
      collision_box = normalize.box(b), orientation = b.orientation or b[3] or 0
    }
  end
  return result
end
companion.cliffs = cliff(assert(data.raw.cliff.cliff, "Missing cliff prototype"))
-- Planet cliffs (Space Age) are listed separately; most reuse the Nauvis geometry.
companion.other_cliffs = {}
for _, name in ipairs(catalog.space_age.cliffs) do
  if data.raw.cliff[name] then companion.other_cliffs[#companion.other_cliffs + 1] = cliff(data.raw.cliff[name]) end
end
data:extend({{type = "mod-data", name = "fsim-export-companion", data_type = "fsim.prototype-companion.v2",
  data = companion}})
