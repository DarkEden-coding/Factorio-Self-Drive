-- Only this adapter reads terrain from the engine. Caches 16x16 blocks; queries
-- extend to include nests whose zones cross the block boundary. Two queues:
-- urgent blocks (around the vehicle, needed for the next decision) are built
-- before background blocks (regions of routes being planned ahead).
local raster = require("raster")
local trains = require("trains")
local M = {}
local function box(b)
  return {{b.left_top.x,b.left_top.y},{b.right_bottom.x,b.right_bottom.y}}
end
-- options.rails: also mark ground-level rails (raster bit 128) for the planner.
function M.new(options)
  return {blocks={},rows={},urgent={},queue={},pending={},rails=options and options.rails or nil}
end
-- Queues every missing block overlapping [x0,x1) x [y0,y1).
function M.ensure(grid,x0,y0,x1,y1,urgent)
  for by = math.floor(y0/16), math.floor((y1-1)/16) do
    for bx = math.floor(x0/16), math.floor((x1-1)/16) do
      local key = bx .. ":" .. by
      local pending = grid.pending[key]
      if not grid.blocks[key] and pending ~= "urgent" and (urgent or not pending) then
        local queue = urgent and grid.urgent or grid.queue
        queue[#queue+1] = {x=bx*16,y=by*16,key=key}
        grid.pending[key] = urgent and "urgent" or "background"
      end
    end
  end
end
function M.dirty(grid,position)
  local r = raster.nest_radius + 8
  for by = math.floor((position.y-r)/16), math.floor((position.y+r)/16) do
    for bx = math.floor((position.x-r)/16), math.floor((position.x+r)/16) do
      local key = bx .. ":" .. by
      if grid.blocks[key] then
        grid.blocks[key] = nil
        if grid.rows[by] then grid.rows[by][bx]=nil end
        M.ensure(grid,bx*16,by*16,bx*16+16,by*16+16,false)
      end
    end
  end
end
-- Tile and obstacle facts by prototype name. Cars and tanks share the default car
-- collision mask; anything meeting it blocks them, like lava or Gleba's deep lakes.
local blocks_cache, friction_cache = {}, {}
local function blocks_vehicles(prototype)
  local cached = blocks_cache[prototype.name]
  if cached == nil then
    local car = prototypes.entity["car"].collision_mask.layers
    cached = false
    for layer in pairs(prototype.collision_mask.layers) do
      if car[layer] then cached = true; break end
    end
    blocks_cache[prototype.name] = cached
  end
  return cached
end
-- vehicle_friction_modifier in tenths, as the simulator raster stores it.
local function friction_tenths(tile)
  local cached = friction_cache[tile.name]
  if not cached then
    cached = math.max(1, math.min(255, math.floor(tile.prototype.vehicle_friction_modifier * 10 + 0.5)))
    friction_cache[tile.name] = cached
  end
  return cached
end
local kinds = {tree="tree",plant="tree",["simple-entity"]="rock",["lightning-attractor"]="rock",
  cliff="cliff",["unit-spawner"]="nest"}
-- Types collected as terrain below (simple entities too, skipped in the building query), or that move
-- and must not be cached as terrain. Any other entity that blocks vehicles,
-- from any mod, is a building.
local IGNORED_TYPES = {"tree","plant","cliff","unit-spawner","lightning-attractor","car","spider-vehicle","character","unit",
  "locomotive","cargo-wagon","fluid-wagon","artillery-wagon","infinity-cargo-wagon","segmented-unit","spider-unit",
  "item-entity","corpse","character-corpse","fish","asteroid","projectile","resource","entity-ghost","tile-ghost"}
-- Area the building query of a block covers.
local function building_area(x0,y0) return {{x0-8,y0-8},{x0+24,y0+24}} end
-- Entities around a block that collide with cars, other than the types
-- skipped above: other mods may create or remove buildings without raising
-- an event, so blocks near the vehicle are recounted while driving
-- (M.changed). Two counts, as an inverted filter would invert the mask too;
-- explosions, smoke and fire collide with nothing and never count.
local function census(surface,x0,y0)
  local area, layers = building_area(x0,y0), prototypes.entity["car"].collision_mask.layers
  return surface.count_entities_filtered({area=area,collision_mask=layers})
    - surface.count_entities_filtered({area=area,collision_mask=layers,type=IGNORED_TYPES})
end
-- Parked carriages around a block. A manual train starts moving without an
-- event, so the recount also compares how many carriages are parked.
local function parked_carriages(surface,x0,y0)
  local out = {}
  for _, carriage in pairs(surface.find_entities_filtered({area=building_area(x0,y0),type=trains.ROLLING_STOCK})) do
    local train = carriage.train
    if train and trains.parked(train) and trains.on_ground(train) then out[#out+1] = carriage end
  end
  return out
end
local function build(grid,surface,next)
  local x0,y0 = next.x,next.y
  local tiles,friction,entities = {},{},{}
  for y = y0,y0+15 do
    for x = x0,x0+15 do
      local tile = surface.get_tile(x,y)
      -- Ungenerated chunks give an invalid tile; treat them like the map edge.
      if not tile.valid or tile.name == "out-of-map" then
        tiles[#tiles+1], friction[#friction+1] = "offmap", 10
      else
        tiles[#tiles+1] = blocks_vehicles(tile.prototype)
        friction[#friction+1] = friction_tenths(tile)
      end
    end
  end
  -- Only nest zones need the wide query. Querying every tree in that area
  -- repeatedly dominates cache construction in forests. Eight tiles covers the
  -- pinned base collision boxes, including the rotated cliff offsets.
  local function collect(types,reach)
    for _, entity in pairs(surface.find_entities_filtered({
      area={{x0-reach,y0-reach},{x0+16+reach,y0+16+reach}},type=types})) do
      if blocks_vehicles(entity.prototype) then
        entities[#entities+1] = {x=entity.position.x,y=entity.position.y,name=entity.name,
          kind=kinds[entity.type],
          orientation=entity.orientation,health=entity.health,
          box=box(entity.prototype.collision_box),
          cliff_orientation=entity.type == "cliff" and entity.cliff_orientation or nil}
      end
    end
  end
  collect({"tree","plant","simple-entity","cliff","lightning-attractor"},8)
  collect({"unit-spawner"},raster.nest_radius+8)
  -- Player structures enter the raster as hard obstacles (the rock channel),
  -- since the trained observation has no separate building channel.
  for _, entity in pairs(surface.find_entities_filtered({
    area=building_area(x0,y0),type=IGNORED_TYPES,invert=true})) do
    if entity.type ~= "simple-entity" and blocks_vehicles(entity.prototype) then
      local b, p = entity.bounding_box, entity.position
      entities[#entities+1] = {x=p.x,y=p.y,kind="building",orientation=0,health=entity.health,
        box={{b.left_top.x-p.x,b.left_top.y-p.y},{b.right_bottom.x-p.x,b.right_bottom.y-p.y}}}
    end
  end
  -- Moving trains are handled while driving (trains.lua), not cached.
  if grid.rails then
    -- Curved rails reach about 4 tiles from their position.
    for _, rail in pairs(surface.find_entities_filtered({
      area={{x0-4,y0-4},{x0+20,y0+20}},type=trains.GROUND_RAILS})) do
      entities[#entities+1] = trains.rail_shape(rail)
    end
  end
  local parked = parked_carriages(surface,x0,y0)
  for _, carriage in ipairs(parked) do entities[#entities+1] = trains.shape(carriage) end
  -- Demolisher territory (Vulcanus) counts as a nest zone; a block lies in one chunk.
  local territory = surface.get_territory_for_chunk({math.floor(x0/32),math.floor(y0/32)}) ~= nil
  local block = raster.build(x0,y0,tiles,entities,territory,friction)
  block.census, block.parked = census(surface,x0,y0), #parked
  grid.blocks[next.key] = block
  local bx,by=math.floor(x0/16),math.floor(y0/16)
  grid.rows[by]=grid.rows[by] or {}
  grid.rows[by][bx]=block
end
-- Builds up to `count` blocks, urgent first. Returns true when both queues are empty.
function M.slice(grid,surface,count)
  for _ = 1, count or 1 do
    local queue = #grid.urgent > 0 and grid.urgent or grid.queue
    local next = table.remove(queue, 1)
    if not next then return true end
    if not grid.blocks[next.key] then build(grid,surface,next) end
    grid.pending[next.key] = nil
  end
  return #grid.urgent == 0 and #grid.queue == 0
end
-- Centres of cached blocks overlapping [x0,x1) x [y0,y1) whose building or
-- parked carriage count changed since they were built (blocks from older versions count as changed).
function M.changed(grid,surface,x0,y0,x1,y1)
  local result = {}
  for by = math.floor(y0/16), math.floor((y1-1)/16) do
    for bx = math.floor(x0/16), math.floor((x1-1)/16) do
      local block = grid.blocks[bx .. ":" .. by]
      if block and (block.census ~= census(surface,block.x0,block.y0) or
        block.parked ~= #parked_carriages(surface,block.x0,block.y0)) then
        result[#result+1] = {x=block.x0+8,y=block.y0+8}
      end
    end
  end
  return result
end
function M.urgent_pending(grid) return #grid.urgent > 0 end
function M.background_pending(grid) return #grid.queue > 0 end
-- Drops cached blocks outside every given rectangle so long drives don't grow storage.
function M.prune(grid,keep)
  for key, block in pairs(grid.blocks) do
    local x, y = block.x0 + 8, block.y0 + 8
    local inside = false
    for _, r in ipairs(keep) do
      if x >= r.x0 - 16 and x < r.x1 + 16 and y >= r.y0 - 16 and y < r.y1 + 16 then inside = true; break end
    end
    if not inside and not grid.pending[key] then
      grid.blocks[key] = nil
      local bx, by = math.floor(block.x0/16), math.floor(block.y0/16)
      if grid.rows[by] then grid.rows[by][bx] = nil end
    end
  end
end
function M.mask(grid,x,y) return raster.mask(grid,x,y) end
return M
