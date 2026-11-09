-- Simulator raster semantics, independent of Factorio handles for parity tests.
local geometry = require("geometry-data")
local M = {nest_radius = geometry.nest_radius, size = 16}
local bor, band = bit32.bor, bit32.band
local function shape(entity)
  local box, orientation = entity.box, entity.orientation or 0
  if entity.kind == "cliff" then
    -- Planet cliffs with their own geometry (crater cliffs) are listed by name.
    local orientations = geometry.other_cliffs and geometry.other_cliffs[entity.name] or geometry.cliffs
    local cliff = assert(orientations[entity.cliff_orientation], "Unknown cliff orientation")
    box, orientation = cliff.collision_box, cliff.orientation
  end
  local c, s = math.cos(orientation * 2 * math.pi), math.sin(orientation * 2 * math.pi)
  local lx, ly = (box[1][1]+box[2][1])/2, (box[1][2]+box[2][2])/2
  return {x=entity.x+c*lx-s*ly,y=entity.y+s*lx+c*ly,
    hx=(box[2][1]-box[1][1])/2,hy=(box[2][2]-box[1][2])/2,c=c,s=s}
end
local function intersects(b,x,y)
  local dx, dy = x+0.5-b.x, y+0.5-b.y
  local ac, as = math.abs(b.c), math.abs(b.s)
  if math.abs(dx) >= b.hx*ac+b.hy*as+0.5-1e-9 then return false end
  if math.abs(dy) >= b.hx*as+b.hy*ac+0.5-1e-9 then return false end
  if math.abs(dx*b.c+dy*b.s) >= b.hx+0.5*(ac+as)-1e-9 then return false end
  if math.abs(-dx*b.s+dy*b.c) >= b.hy+0.5*(ac+as)-1e-9 then return false end
  return true
end
-- `territory` marks a whole block as zone (demolisher territory). `friction` holds
-- vehicle_friction_modifier tenths per tile (10 when omitted).
function M.build(x0,y0,tiles,entities,territory,friction)
  local block = {x0=x0,y0=y0,mask={},nests={},territory=territory or false,friction=friction}
  for i = 1,256 do
    block.mask[i] = (tiles[i] and 17 or 0) + ((territory and tiles[i] ~= "offmap") and 32 or 0)
  end
  for _, e in ipairs(entities) do
    if e.kind == "cliff" or (e.health or 1) > 0 then
      local b = shape(e)
      local bits = 1 + ({cliff=2,tree=4,rock=8,nest=64})[e.kind]
      local ex = math.abs(b.c)*b.hx+math.abs(b.s)*b.hy
      local ey = math.abs(b.s)*b.hx+math.abs(b.c)*b.hy
      for y = math.max(y0, math.floor(b.y-ey)), math.min(y0+15, math.ceil(b.y+ey)-1) do
        for x = math.max(x0, math.floor(b.x-ex)), math.min(x0+15, math.ceil(b.x+ex)-1) do
          if tiles[(y-y0)*16+x-x0+1] ~= "offmap" and intersects(b,x,y) then
            local i = (y-y0)*16+x-x0+1
            block.mask[i] = bor(block.mask[i],bits)
          end
        end
      end
      if e.kind == "nest" then
        block.nests[#block.nests+1] = {x=e.x,y=e.y}
        for y = y0,y0+15 do
          for x = x0,x0+15 do
            if tiles[(y-y0)*16+x-x0+1] ~= "offmap" and (x+0.5-e.x)^2+(y+0.5-e.y)^2 <= geometry.nest_radius^2 then
              local i = (y-y0)*16+x-x0+1
              block.mask[i] = bor(block.mask[i],32)
            end
          end
        end
      end
    end
  end
  return block
end
function M.key(x,y) return math.floor(x/16) .. ":" .. math.floor(y/16) end
function M.mask(grid,x,y)
  local row = grid.rows and grid.rows[math.floor(y/16)]
  local block = grid.rows and row and row[math.floor(x/16)] or
    (not grid.rows and grid.blocks[M.key(x,y)])
  if not block then return 17 end
  return block.mask[(y-block.y0)*16+x-block.x0+1]
end
-- Like mask, but blocks not cached yet read as open ground (0).
function M.known_mask(grid,x,y)
  local row = grid.rows and grid.rows[math.floor(y/16)]
  local block = grid.rows and row and row[math.floor(x/16)] or
    (not grid.rows and grid.blocks[M.key(x,y)])
  if not block then return 0 end
  return block.mask[(y-block.y0)*16+x-block.x0+1]
end
local function block_at(grid,x,y)
  local row = grid.rows and grid.rows[math.floor(y/16)]
  return grid.rows and row and row[math.floor(x/16)] or
    (not grid.rows and grid.blocks[M.key(x,y)])
end
-- Friction modifier in tenths at a tile; 10 (modifier 1) where unknown.
function M.friction(grid,x,y)
  local block = block_at(grid,x,y)
  if not block or not block.friction then return 10 end
  return block.friction[(y-block.y0)*16+x-block.x0+1]
end
function M.in_zone(grid,x,y)
  local block = block_at(grid,x,y)
  if block and block.territory then return 1 end
  if block then
    for _, n in ipairs(block.nests) do
      if (x-n.x)^2+(y-n.y)^2 <= geometry.nest_radius^2 then return 1 end
    end
  end
  return 0
end
function M.hard(mask) return band(mask,90) ~= 0 or band(mask,5) == 1 end
return M
