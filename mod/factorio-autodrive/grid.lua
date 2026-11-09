-- Only this adapter reads terrain from the engine. Caches 16x16 blocks; queries
-- extend to include nests whose zones cross the block boundary. Two queues:
-- urgent blocks (around the vehicle, needed for the next decision) are built
-- before background blocks (regions of routes being planned ahead).
local raster = require("raster")
local M = {}
local function box(b)
  return {{b.left_top.x,b.left_top.y},{b.right_bottom.x,b.right_bottom.y}}
end
function M.new()
  return {blocks={},rows={},urgent={},queue={},pending={}}
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
  -- Demolisher territory (Vulcanus) counts as a nest zone; a block lies in one chunk.
  local territory = surface.get_territory_for_chunk({math.floor(x0/32),math.floor(y0/32)}) ~= nil
  local block = raster.build(x0,y0,tiles,entities,territory,friction)
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
