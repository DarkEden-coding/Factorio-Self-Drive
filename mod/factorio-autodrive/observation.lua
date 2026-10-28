local raster = require("raster")
local u = require("math-util")
local weights = require("weights")
local spec, map_spec = weights.obs, weights.map
local band = bit32.band
local M = {}
local function cast(grid,x,y,dx,dy,max_t)
  local ix,iy = math.floor(x),math.floor(y)
  local sx,sy = dx > 0 and 1 or -1,dy > 0 and 1 or -1
  local delta_x = math.abs(dx)>1e-12 and 1/math.abs(dx) or math.huge
  local delta_y = math.abs(dy)>1e-12 and 1/math.abs(dy) or math.huge
  -- Parallel rays starting on a tile boundary must not evaluate 0 * infinity.
  local nx = delta_x<math.huge and (dx>0 and ix+1-x or x-ix)*delta_x or math.huge
  local ny = delta_y<math.huge and (dy>0 and iy+1-y or y-iy)*delta_y or math.huge
  local t,tree,zone = 0,max_t,max_t
  while t < max_t do
    local mask = raster.mask(grid,ix,iy)
    if raster.hard(mask) then return t,math.min(tree,t),math.min(zone,t) end
    if bit32.band(mask,4)~=0 and tree==max_t then tree=t end
    if bit32.band(mask,32)~=0 and zone==max_t then zone=t end
    if nx < ny then t,nx,ix=nx,nx+delta_x,ix+sx
    else t,ny,iy=ny,ny+delta_y,iy+sy end
  end
  return max_t,tree,zone
end
-- Coarse vehicle-frame map (student_obs.rs write_map): cells x cells cells of
-- cell_tiles tiles, row 1 ahead, column 1 left; per channel (hard, tree, zone) the
-- fraction of the cell's samples x samples points of that kind, channel-major.
-- Blocks not cached yet read as open ground rather than as the off-map wall.
local function write_map(grid,x,y,h,out)
  local cells, tiles, samples = map_spec.cells, map_spec.cell_tiles, map_spec.samples
  local s, c = math.sin(h), math.cos(h)
  local half, step, weight = cells/2, tiles/samples, 1/(samples*samples)
  local plane, base = cells*cells, #out
  for i = 1, 3*plane do out[base+i] = 0 end
  for row = 0, cells-1 do
    for col = 0, cells-1 do
      local cell = base + row*cells + col + 1
      local hard, tree, zone = 0, 0, 0
      for sy = 0, samples-1 do
        local forward = (half-row)*tiles - (sy+0.5)*step
        for sx = 0, samples-1 do
          local right = (col-half)*tiles + (sx+0.5)*step
          local mask = raster.known_mask(grid,math.floor(x+right*c+forward*s),math.floor(y+right*s-forward*c))
          if raster.hard(mask) then hard = hard + 1 end
          if band(mask,4) ~= 0 then tree = tree + 1 end
          if band(mask,32) ~= 0 then zone = zone + 1 end
        end
      end
      out[cell], out[cell+plane], out[cell+2*plane] = hard*weight, tree*weight, zone*weight
    end
  end
end
-- state is a plain snapshot. stats are in simulator units (W, tiles/tick).
function M.build(grid,route,state,stats)
  local out,look = {},{}
  local x,y,h = state.x,state.y,state.orientation*2*math.pi
  local s,c = math.sin(h),math.cos(h)
  local hw,hl = stats.half_width,stats.half_length
  for i,deg in ipairs(spec.ray_angles_deg) do
    local a = deg*math.pi/180
    local sa,ca = math.abs(math.sin(a)),math.abs(math.cos(a))
    local extent = math.min(sa>1e-9 and hw/sa or math.huge,ca>1e-9 and hl/ca or math.huge)
    local hard,tree,zone = cast(grid,x,y,math.sin(h+a),-math.cos(h+a),extent+32)
    out[i],out[i+32],out[i+64] = u.clamp(hard-extent,0,32)/32,
      u.clamp(tree-extent,0,32)/32,u.clamp(zone-extent,0,32)/32
  end
  local function local_point(p)
    return (p.x-x)*c+(p.y-y)*s,(p.x-x)*s-(p.y-y)*c
  end
  for _,distance in ipairs(spec.route_lookaheads) do
    local p = u.point_along(route,distance)
    look[#look+1] = p
    local right,forward = local_point(p)
    out[#out+1],out[#out+2] = right/distance,forward/distance
  end
  out[#out+1] = u.clamp(u.length(route)/64,0,8)
  local right,forward = local_point(state.target)
  local distance = math.sqrt(right^2+forward^2)
  local shared = {distance>1e-9 and right/distance or 0,distance>1e-9 and forward/distance or 0,
    u.clamp(distance/64,0,8),3/8,state.speed*216/100,state.health/stats.max_health,
    stats.tank_driving and 1 or 0,stats.weight/20000,stats.max_health/2000,
    stats.rotation_speed/0.015,stats.braking_power/1e6,stats.power/stats.weight/100,
    stats.top_speed*216/100,hw,hl,stats.fuel_acceleration,stats.fuel_top_speed,stats.tile_friction-1}
  for _,v in ipairs(shared) do out[#out+1] = v end
  for a = 0,8 do out[#out+1] = state.previous_action == a and 1 or 0 end
  out[#out+1] = state.elapsed/state.time_limit
  out[#out+1] = state.stall/1200
  out[#out+1] = raster.in_zone(grid,x,y)
  assert(#out == spec.size,"Student observation layout mismatch")
  if map_spec then
    write_map(grid,x,y,h,out)
    assert(#out == map_spec.size,"Student map layout mismatch")
  end
  for i,v in ipairs(out) do out[i] = u.f32(v) end
  return out,look
end
return M
