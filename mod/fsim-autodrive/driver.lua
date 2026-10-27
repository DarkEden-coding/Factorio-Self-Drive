-- Drives to a target with the pathfinder-route map student (docs/autodrive-mod.md).
-- The route features come from the game's own pathfinder (LuaSurface.request_path,
-- followed as in training by path.lua); the coarse map around the vehicle shows
-- obstacles and nest zones the pathfinder ignores. Driving starts as soon as the
-- blocks under the rays are cached and the first path answer is in (or after
-- FIRST_PATH_WAIT ticks, heading straight for the target meanwhile).
local grid = require("grid")
local raster = require("raster")
local path = require("path")
local observation = require("observation")
local network = require("network")
local vehicle_api = require("vehicle")
local u = require("math-util")
local M = {}

local RAY_REACH = 36         -- tiles around the vehicle the rays need (urgent blocks)
local MAP_REACH = 92         -- tiles around the vehicle the map covers (128 x 128 rotated)
local PLAN_BLOCKS = 12       -- 16x16 cache blocks built per tick while stopped
local DRIVE_BLOCKS = 4       -- and while driving (forest blocks are costly)
local FIRST_PATH_WAIT = 120  -- ticks to wait for the first path before driving without one
local MAX_TARGET = 4096

local function is_drivable(g,x,y,tank)
  local m = raster.mask(g,x,y)
  return not raster.hard(m) and (tank or bit32.band(m,4) == 0)
end
-- Nearest tile center within 8 tiles of p that passes `ok`, or nil.
local function snap(g,p,ok)
  local cx, cy = math.floor(p.x), math.floor(p.y)
  for r = 0, 8 do
    local best, bd
    for y = cy - r, cy + r do
      for x = cx - r, cx + r do
        if (math.abs(x - cx) == r or math.abs(y - cy) == r) and ok(g,x,y) then
          local d = (x + 0.5 - p.x)^2 + (y + 0.5 - p.y)^2
          if not bd or d < bd then best, bd = {x=x+0.5,y=y+0.5}, d end
        end
      end
    end
    if best then return best end
  end
end
local function generated(surface,p)
  return surface.is_chunk_generated({math.floor(p.x/32),math.floor(p.y/32)})
end
local function around(s,p,reach,urgent)
  grid.ensure(s.grid,math.floor(p.x)-reach,math.floor(p.y)-reach,math.floor(p.x)+reach+1,
    math.floor(p.y)+reach+1,urgent)
end

function M.start(vehicle,target)
  assert(vehicle.valid and (vehicle.name=="car" or vehicle.name=="tank"),"Enter a base car or tank first")
  assert(u.distance(vehicle.position,target)<=MAX_TARGET,"Target must be within "..MAX_TARGET.." tiles")
  assert(generated(vehicle.surface,target),"Target must be in an explored (generated) chunk")
  local s = {vehicle=vehicle,target={x=target.x,y=target.y},grid=grid.new(),status="planning",
    elapsed=0,stall=0,best_tick=0,decisions=0,slices=0,planning_ticks=0,action=4,route={},
    stats=vehicle_api.stats(vehicle)}
  -- Check the clicked target first, then the blocks the first decision reads.
  grid.ensure(s.grid,s.target.x-12,s.target.y-12,s.target.x+13,s.target.y+13,true)
  around(s,vehicle.position,RAY_REACH,true)
  around(s,vehicle.position,MAP_REACH,false)
  return s
end

-- Called from control.lua's on_script_path_request_finished.
function M.on_path(s,event)
  if not s.path or not path.on_path(s.path,event) or not event.path then return end
  -- A new path restarts the progress measure along it (the stall timer keeps running),
  -- and the time limit grows to cover the new remaining length.
  local remaining = u.length(s.path.waypoints)
  s.best_distance = remaining
  if s.time_limit then s.time_limit = math.max(s.time_limit,s.elapsed+math.ceil(600+12*remaining)) end
end

function M.snapshot(s)
  local v = s.vehicle
  return {x=v.position.x,y=v.position.y,orientation=v.orientation,speed=v.speed,health=v.health,
    target=s.target,elapsed=s.elapsed,stall=s.stall,time_limit=s.time_limit,
    previous_action=s.previous_action}
end
local function profile(s,kind,profiler)
  profiler.stop()
  -- Never put profiler objects or wall-clock measurements in storage or use
  -- them for simulation decisions. Local output only, safe in multiplayer.
  if s.profile then helpers.write_file("fsim-autodrive/timing.txt",{"",kind," ",profiler,"\n"},true) end
end
function M.observe(s)
  local state = M.snapshot(s)
  s.stats = vehicle_api.stats(s.vehicle)
  local obs,look = observation.build(s.grid,s.route,state,s.stats)
  local action,logits = network.forward(obs)
  s.look = look
  return action,{state=state,stats=s.stats,observation=obs,logits=logits,action=action,route=s.route,
    spec_hash=network.model.spec_hash,waypoints=s.path and s.path.waypoints}
end
function M.dump(s,path_name)
  assert(s.status=="driving","Not driving yet")
  local _,sample = M.observe(s)
  sample.grid = s.grid.blocks
  helpers.write_file(path_name,helpers.table_to_json(sample),false)
end
local function finish(s,status)
  s.status = status
  vehicle_api.brake(s.vehicle)
  return status
end

local function plan(s,v,profiler)
  s.planning_ticks = s.planning_ticks + 1
  vehicle_api.brake(v)
  if grid.urgent_pending(s.grid) then profile(s,"cache",profiler); return end
  if not s.path then
    local t, tank = s.target, s.stats.tank_driving
    local function drivable(g,x,y) return is_drivable(g,x,y,tank) end
    if not drivable(s.grid,math.floor(t.x),math.floor(t.y)) then
      -- A clicked target on water, a rock or (for the car) a tree moves to the
      -- nearest drivable tile.
      local snapped = snap(s.grid,t,drivable)
      if not snapped then return finish(s,"unreachable (no drivable ground near the target)") end
      s.target = snapped
    end
    s.path = path.new(s.target)
    path.request(s.path,v)
  end
  local p = s.path
  if p.request and s.planning_ticks < FIRST_PATH_WAIT then profile(s,"route",profiler); return end
  -- Path in, failed, or late: drive (a straight line stands in until a path arrives).
  s.route = path.route(p,v)
  local remaining = u.length(s.route)
  s.best_distance, s.time_limit = remaining, math.max(1,math.ceil(600+12*remaining))
  s.elapsed, s.best_tick, s.stall, s.started = 0, 0, 0, false
  s.status, s.previous_action = "driving", nil
  profile(s,"route",profiler)
end

function M.tick(s)
  local v = s.vehicle
  if not v.valid then s.status="destroyed"; return "destroyed" end
  if s.status == "paused" then return end
  if s.status ~= "planning" and s.status ~= "driving" then vehicle_api.brake(v); return end
  local profiler = helpers.create_profiler()
  if s.status == "planning" then
    grid.slice(s.grid,v.surface,PLAN_BLOCKS)
    return plan(s,v,profiler)
  end
  grid.slice(s.grid,v.surface,DRIVE_BLOCKS)
  -- Engine-tick arrival check at the target.
  if u.distance(v.position,s.target)<=1 and math.abs(v.speed)<=0.01 then return finish(s,"arrived") end
  s.elapsed=s.elapsed+(s.started and 1 or 0)
  s.started=true
  -- Rays need cached blocks around the vehicle: wait only for those. The map's
  -- wider area fills in the background (uncached blocks read as open ground).
  around(s,v.position,RAY_REACH,true)
  if game.tick % 16 == 0 then around(s,v.position,MAP_REACH,false) end
  if grid.urgent_pending(s.grid) then
    grid.slice(s.grid,v.surface,DRIVE_BLOCKS)
    if grid.urgent_pending(s.grid) then
      vehicle_api.brake(v)
      profile(s,"cache",profiler)
      return
    end
  end
  if (s.elapsed-1)%4==0 then
    s.route = path.route(s.path,v)
    local remaining = u.length(s.route)
    if remaining<s.best_distance-1 then s.best_distance,s.best_tick=remaining,s.elapsed end
    s.stall=s.elapsed-s.best_tick
    if s.elapsed>=s.time_limit then return finish(s,"timeout") end
    if s.stall>=1200 then return finish(s,"stalled") end
    local action=M.observe(s)
    s.action,s.previous_action,s.decisions=action,action,s.decisions+1
    profile(s,"decision",profiler)
  end
  if game.tick % 300 == 0 then
    local p = v.position
    grid.prune(s.grid,{{x0=p.x-MAP_REACH-16,y0=p.y-MAP_REACH-16,x1=p.x+MAP_REACH+17,y1=p.y+MAP_REACH+17},
      {x0=s.target.x-16,y0=s.target.y-16,x1=s.target.x+17,y1=s.target.y+17}})
  end
  -- Re-evaluate brake/reverse mapping each tick across a speed sign change.
  vehicle_api.apply(v,s.action)
end
return M
