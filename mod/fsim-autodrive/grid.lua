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
local function build(grid,surface,next)
  local x0,y0 = next.x,next.y
  local tiles,entities = {},{}
  for y = y0,y0+15 do
    for x = x0,x0+15 do
      local tile = surface.get_tile(x,y)
      tiles[#tiles+1] = tile.name == "out-of-map" and "offmap" or tile.collides_with("water_tile")
    end
  end
  local reach = raster.nest_radius + 8
  for _, entity in pairs(surface.find_entities_filtered({
    area={{x0-reach,y0-reach},{x0+16+reach,y0+16+reach}},
    type={"tree","simple-entity","cliff","unit-spawner"}})) do
    if entity.type ~= "simple-entity" or entity.prototype.count_as_rock_for_filtered_deconstruction then
      entities[#entities+1] = {x=entity.position.x,y=entity.position.y,
        kind=({tree="tree",["simple-entity"]="rock",cliff="cliff",["unit-spawner"]="nest"})[entity.type],
        orientation=entity.orientation,health=entity.health,
        box=box(entity.prototype.collision_box),
        cliff_orientation=entity.type == "cliff" and entity.cliff_orientation or nil}
    end
  end
  local block = raster.build(x0,y0,tiles,entities)
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
