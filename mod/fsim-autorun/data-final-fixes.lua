-- Keep graphics separate from the physics companion. PNGs remain in the local install.
local graphics = {schema_version = 1, tiles = {}, entities = {}, cliffs = {}, resources = {}, vehicles = {}}
for name, tile in pairs(data.raw.tile) do
  if tile.autoplace then graphics.tiles[name] = {variants = tile.variants} end
end
for name, tree in pairs(data.raw.tree) do
  graphics.entities[name] = {kind = "tree", variations = tree.variations, pictures = tree.pictures,
    colors = tree.colors}
end
for name, rock in pairs(data.raw["simple-entity"]) do
  if rock.count_as_rock_for_filtered_deconstruction then
    graphics.entities[name] = {kind = "rock", pictures = rock.pictures, picture = rock.picture}
  end
end
for name, nest in pairs(data.raw["unit-spawner"]) do
  graphics.entities[name] = {kind = "nest", animations = nest.graphics_set.animations}
end
for name, cliff in pairs(data.raw.cliff) do
  graphics.cliffs[name] = cliff.orientations
end
for name, resource in pairs(data.raw.resource) do
  graphics.resources[name] = {stages = resource.stages, stage_counts = resource.stage_counts}
end
for _, name in ipairs({"car", "tank"}) do
  local car = data.raw.car[name]
  graphics.vehicles[name] = {animation = car.animation, turret_animation = car.turret_animation}
end
data:extend({{type = "mod-data", name = "fsim-graphics", data_type = "fsim.graphics.v1", data = graphics}})
