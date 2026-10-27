-- Drives toward a far target in segments. Each segment is a simulator-style
-- distance field to a sub-target; the student sees that segment exactly as in
-- training. The first segment is short so driving starts within a fraction of a
-- second; later segments are planned in the background while driving and take
-- over before the current sub-target is reached. A coarse engine path
-- (LuaSurface.request_path) picks sub-targets around large obstacles; until it
-- arrives (or if it fails) sub-targets lie on the straight line to the target.
local field = require("field")
local grid = require("grid")
local raster = require("raster")
local observation = require("observation")
local network = require("network")
local vehicle_api = require("vehicle")
local u = require("math-util")
local M = {}

local FIRST_SEGMENT = 48     -- tiles to the first sub-target
local SEGMENT = 160          -- tiles to each later sub-target
local FINAL_WITHIN = 200     -- the final target becomes the sub-target when this close
local SINGLE_WITHIN = 64     -- targets this close are planned as one segment, like training
local MARGIN = 32            -- field margin around a segment
local FINAL_MARGIN = 48      -- the simulator's margin, for single-segment trips
local HANDOFF = 50           -- switch to the next segment with this much route left
local FIELD_BUDGET = 4000    -- field work units per tick (~1-2 ms)
local GRID_BLOCKS = 4        -- 16x16 cache blocks built per tick (forest blocks are costly)
local MAX_TARGET = 4096

-- Sub-targets must be clear of trees and nest zones too; the final target only
-- has to be drivable (the player may pick a spot inside a nest zone).
local function is_free(g,x,y)
  local m = raster.mask(g,x,y)
  return not raster.hard(m) and bit32.band(m,4) == 0 and bit32.band(m,32) == 0
end
local function is_drivable(g,x,y,tank)
  local m = raster.mask(g,x,y)
  return not raster.hard(m) and (tank or bit32.band(m,4) == 0)
end
-- Nearest tile center within 8 tiles of p that passes `ok`, or nil.
local function snap(g,p,ok)
  ok = ok or is_free
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

-- Next sub-target after `from`, and whether it is the final target.
local function pick(s,from,length)
  if u.distance(from,s.target) <= FINAL_WITHIN and length >= SEGMENT then return s.target, true end
  if u.distance(from,s.target) <= length then return s.target, true end
  local p
  if s.path then
    -- Nearest coarse waypoint to `from`, then `length` tiles further along the path.
    local best, bi = math.huge, 1
    for i = s.path_cursor or 1, #s.path do
      local d = u.distance(from,s.path[i])
      if d < best then best, bi = d, i end
    end
    s.path_cursor = bi
    local rest = {from}
    for i = bi + 1, #s.path do rest[#rest+1] = s.path[i] end
    rest[#rest+1] = s.target
    p = u.point_along(rest,length)
  else
    local d = u.distance(from,s.target)
    p = {x=from.x+(s.target.x-from.x)*length/d,y=from.y+(s.target.y-from.y)*length/d}
  end
  return p, false
end

local function new_plan(s,from,target,final,margin)
  local stats = s.stats
  local f = field.new(from,target,stats.half_width,stats.tank_driving,nil,margin)
  grid.ensure(s.grid,f.x0-3,f.y0-3,f.x1+3,f.y1+3,false)
  return {from=from,target=target,aim=target,final=final,margin=margin,field=f,phase="grid",attempt=0}
end
-- Advances a plan by one tick of work. Returns "ready", "failed" or nil.
local function step_plan(s,p,probe)
  if p.phase == "grid" then
    if grid.background_pending(s.grid) then return end
    local t, tank = p.target, s.stats.tank_driving
    if p.final then
      -- A clicked target on water, a rock or (for the car) a tree moves to the
      -- nearest drivable tile.
      if not is_drivable(s.grid,math.floor(t.x),math.floor(t.y),tank) then
        local snapped = snap(s.grid,t,function(g,x,y) return is_drivable(g,x,y,tank) end)
        if not snapped then p.phase = "unreachable"; return "failed" end
        p.target, p.field.target, s.target = snapped, snapped, snapped
      end
    else
      local snapped = snap(s.grid,t)
      if not snapped then p.phase = "unreachable"; return "failed" end
      p.target, p.field.target = snapped, snapped
    end
    p.phase = "field"
  end
  if p.phase == "field" then
    if not field.slice(p.field,function(x,y) return raster.mask(s.grid,x,y) end,FIELD_BUDGET) then return end
    if field.sample(p.field,probe.x,probe.y) == math.huge then p.phase = "unreachable"; return "failed" end
    p.phase = "ready"
    return "ready"
  end
  if p.phase == "ready" then return "ready" end
  return "failed"
end
-- A failed plan is retried in order: the same target with a 64-tile margin,
-- sub-targets rotated 30/60/90 degrees either side of the blocked direction,
-- then closer sub-targets (halving, never under 24 tiles). Returns nil when
-- nothing is left to try.
local TURNS = {30,-30,60,-60,90,-90}
local function retry(s,p)
  local n = p.attempt + 1
  if n == 1 and p.margin >= 64 then n = 2 end
  local length = u.distance(p.from,p.aim)
  local q
  if n == 1 then
    q = new_plan(s,p.from,p.target,p.final,64)
  elseif n <= 1 + #TURNS then
    local dx, dy = (p.aim.x - p.from.x) / math.max(length,1e-9), (p.aim.y - p.from.y) / math.max(length,1e-9)
    local a = TURNS[n - 1] * math.pi / 180
    local c, sn = math.cos(a), math.sin(a)
    q = new_plan(s,p.from,{x=p.from.x+(dx*c-dy*sn)*length,y=p.from.y+(dx*sn+dy*c)*length},false,MARGIN)
  else
    length = length / 2 ^ (n - 1 - #TURNS)
    if length < 24 then return nil end
    local target, final = pick(s,p.from,length)
    q = new_plan(s,p.from,target,final,MARGIN)
  end
  q.attempt, q.aim, q.probe = n, p.aim, p.probe
  return q
end

local function request_path(s)
  local v = s.vehicle
  local ok, id = pcall(v.surface.request_path,{
    bounding_box=v.prototype.collision_box,collision_mask=v.prototype.collision_mask,
    start=v.position,goal=s.target,force=v.force,radius=4,entity_to_ignore=v,
    pathfind_flags={cache=false,prefer_straight_paths=true,no_break=true}})
  if ok then s.path_request = id else s.path_failed = true end
end
local PATH_RETRIES = 3
-- Called from control.lua's on_script_path_request_finished.
function M.on_path(s,event)
  if s.path_request ~= event.id then return end
  s.path_request = nil
  if event.path then
    s.path = {}
    for _, w in ipairs(event.path) do s.path[#s.path+1] = {x=w.position.x,y=w.position.y} end
  elseif event.try_again_later and (s.path_retries or 0) < PATH_RETRIES then
    s.path_retries = (s.path_retries or 0) + 1
    s.path_retry_at = game.tick + 60
  else
    s.path_failed = true
  end
end

function M.start(vehicle,target)
  assert(vehicle.valid and (vehicle.name=="car" or vehicle.name=="tank"),"Enter a base car or tank first")
  assert(u.distance(vehicle.position,target)<=MAX_TARGET,"Target must be within "..MAX_TARGET.." tiles")
  assert(generated(vehicle.surface,target),"Target must be in an explored (generated) chunk")
  local s = {vehicle=vehicle,target={x=target.x,y=target.y},grid=grid.new(),status="planning",
    elapsed=0,stall=0,best_tick=0,decisions=0,slices=0,planning_ticks=0,action=4,route={},segments=0,
    stats=vehicle_api.stats(vehicle)}
  local from = {x=vehicle.position.x,y=vehicle.position.y}
  -- Check the clicked target before anything else; and bound the number of
  -- segments so no failure mode can circle forever.
  grid.ensure(s.grid,s.target.x-12,s.target.y-12,s.target.x+13,s.target.y+13,true)
  s.max_segments = math.ceil(u.distance(from,s.target) / FIRST_SEGMENT) + 8
  if u.distance(from,s.target) <= SINGLE_WITHIN then
    s.plan = new_plan(s,from,s.target,true,FINAL_MARGIN)
  else
    request_path(s)
    local sub, final = pick(s,from,FIRST_SEGMENT)
    s.plan = new_plan(s,from,sub,final,MARGIN)
  end
  -- Compatibility: tests and dumps read s.field (the plan's field until a segment is active).
  s.field = s.plan.field
  return s
end

-- Makes plan p the active segment from the vehicle's current position.
local function activate(s,p)
  s.field, s.seg_target, s.final_segment = p.field, p.target, p.final
  local v = s.vehicle
  local distance = field.sample(p.field,v.position.x,v.position.y)
  s.best_distance,s.time_limit = distance,math.max(1,math.ceil(600+12*distance))
  s.elapsed,s.best_tick,s.stall,s.started = 0,0,0,false
  s.segments = s.segments + 1
  s.plan, s.next = nil, nil
end

function M.snapshot(s)
  local v = s.vehicle
  return {x=v.position.x,y=v.position.y,orientation=v.orientation,speed=v.speed,health=v.health,
    target=s.seg_target or s.target,elapsed=s.elapsed,stall=s.stall,time_limit=s.time_limit,
    previous_action=s.previous_action}
end
local function profile(s,kind,profiler)
  profiler.stop()
  -- Never put profiler objects or wall-clock measurements in storage or use
  -- them for simulation decisions. Local output only, safe in multiplayer.
  if s.profile then helpers.write_file("fsim-autodrive/timing.txt",{"",kind," ",profiler,"\n"},true) end
end
function M.observe(s)
  s.route = field.route(s.field,s.vehicle.position)
  local state = M.snapshot(s)
  s.stats = vehicle_api.stats(s.vehicle)
  local obs,look = observation.build(s.grid,s.route,state,s.stats)
  local action,logits = network.forward(obs)
  s.look = look
  return action,{state=state,stats=s.stats,observation=obs,logits=logits,action=action,route=s.route,
    spec_hash=network.model.spec_hash,segment=s.segments,final_segment=s.final_segment}
end
function M.dump(s,path)
  assert(s.field.phase=="ready","Route is still computing")
  local _,sample = M.observe(s)
  sample.grid = s.grid.blocks
  sample.field_region = {x0=s.field.x0,y0=s.field.y0,x1=s.field.x1,y1=s.field.y1}
  helpers.write_file(path,helpers.table_to_json(sample),false)
end
local function finish(s,status)
  s.status = status
  vehicle_api.brake(s.vehicle)
  return status
end

-- Background planning of the segment after the active one.
local function plan_ahead(s,v)
  if s.final_segment then return end
  if not s.next then
    -- The next field must cover where the vehicle will be at the handoff.
    local route = field.route(s.field,v.position)
    local from = u.point_along(route,math.max(0,u.length(route)-HANDOFF-20))
    local target, final = pick(s,s.seg_target,SEGMENT)
    s.next = new_plan(s,{x=from.x,y=from.y},target,final,final and FINAL_MARGIN or MARGIN)
    -- The next field must reach the point where the handoff happens.
    s.next.probe = s.next.from
  end
  local p = s.next
  if p.phase == "ready" or p.phase == "exhausted" then return end
  local result = step_plan(s,p,p.probe)
  if result == "failed" then
    -- If nothing is left to try, the vehicle stops at the current sub-target and
    -- the stall timer ends the trip there.
    s.next = retry(s,p) or {phase="exhausted"}
  end
end

local function keep_regions(s,v)
  local keep = {{x0=v.position.x-48,y0=v.position.y-48,x1=v.position.x+49,y1=v.position.y+49}}
  for _, p in ipairs({s.field, s.next and s.next.field, s.plan and s.plan.field}) do
    if p then keep[#keep+1] = {x0=p.x0-3,y0=p.y0-3,x1=p.x1+3,y1=p.y1+3} end
  end
  return keep
end

function M.tick(s)
  local v = s.vehicle
  if not v.valid then s.status="destroyed"; return "destroyed" end
  if s.status == "paused" then return end
  if s.status ~= "planning" and s.status ~= "driving" then vehicle_api.brake(v); return end
  local profiler = helpers.create_profiler()
  grid.slice(s.grid,v.surface,GRID_BLOCKS)
  if s.status=="planning" then
    s.planning_ticks=s.planning_ticks+1
    vehicle_api.brake(v)
    local here = {x=v.position.x,y=v.position.y}
    if not s.target_checked then
      if grid.urgent_pending(s.grid) then profile(s,"route",profiler); return end
      s.target_checked = true
      local t, tank = s.target, s.stats.tank_driving
      local function drivable(g,x,y) return is_drivable(g,x,y,tank) end
      if not drivable(s.grid,math.floor(t.x),math.floor(t.y)) then
        local snapped = snap(s.grid,t,drivable)
        if not snapped then return finish(s,"unreachable (no drivable ground near the target)") end
        s.target = snapped
        if s.plan.final then s.plan.target, s.plan.aim, s.plan.field.target = snapped, snapped, snapped end
      end
    end
    if s.path_retry_at and game.tick >= s.path_retry_at then s.path_retry_at = nil; request_path(s) end
    if not s.plan then
      -- Straight-line sub-targets failed; wait for the coarse path, then pick along it.
      if s.path_request then profile(s,"route",profiler); return end
      if not s.path then return finish(s,"unreachable") end
      local target, final = pick(s,here,FIRST_SEGMENT)
      s.plan, s.retried_on_path = new_plan(s,here,target,final,MARGIN), true
      s.field = s.plan.field
    end
    local p = s.plan
    local result = step_plan(s,p,here)
    s.slices=s.slices+1
    profile(s,"route",profiler)
    if result == "ready" then
      activate(s,p)
      s.status,s.previous_action = "driving",nil
    elseif result == "failed" then
      local q = retry(s,p)
      if q then s.plan, s.field = q, q.field
      elseif (s.path_request or s.path) and not s.retried_on_path then s.plan = nil
      else return finish(s,"unreachable") end
    end
    if s.planning_ticks>=3600 then return finish(s,"planning-timeout") end
    return
  end
  -- Engine-tick arrival check, only at the final target.
  if s.final_segment and u.distance(v.position,s.target)<=1 and math.abs(v.speed)<=0.01 then
    return finish(s,"arrived")
  end
  if s.path_retry_at and game.tick >= s.path_retry_at then s.path_retry_at = nil; request_path(s) end
  plan_ahead(s,v)
  if s.next and s.next.phase == "exhausted" and math.abs(v.speed) <= 0.01 and
    u.distance(v.position,s.seg_target) <= 5 then
    return finish(s,"unreachable")
  end
  -- Hand over to the next segment before this sub-target is reached.
  if s.segments > s.max_segments then return finish(s,"unreachable (too many detours)") end
  if s.next and s.next.phase == "ready" and game.tick % 4 == 0 then
    local remaining = u.length(field.route(s.field,v.position))
    if remaining <= HANDOFF and field.sample(s.next.field,v.position.x,v.position.y) < math.huge then
      activate(s,s.next)
    end
  end
  s.elapsed=s.elapsed+(s.started and 1 or 0)
  s.started=true
  local distance=field.sample(s.field,v.position.x,v.position.y)
  if distance<math.huge and distance<s.best_distance-1 then s.best_distance,s.best_tick=distance,s.elapsed end
  s.stall=s.elapsed-s.best_tick
  if s.elapsed>=s.time_limit then return finish(s,"timeout") end
  if s.stall>=1200 then return finish(s,"stalled") end
  -- Rays need cached blocks around the vehicle. Wait only for those.
  grid.ensure(s.grid,math.floor(v.position.x)-36,math.floor(v.position.y)-36,
    math.floor(v.position.x)+37,math.floor(v.position.y)+37,true)
  if grid.urgent_pending(s.grid) then
    grid.slice(s.grid,v.surface,GRID_BLOCKS)
    if grid.urgent_pending(s.grid) then
      vehicle_api.brake(v)
      profile(s,"cache",profiler)
      return
    end
  end
  if v.position.x<s.field.x0+1 or v.position.x>=s.field.x1-1 or
    v.position.y<s.field.y0+1 or v.position.y>=s.field.y1-1 then
    return finish(s,"outside-route-region")
  end
  if (s.elapsed-1)%4==0 then
    local action=M.observe(s)
    s.action,s.previous_action,s.decisions=action,action,s.decisions+1
    profile(s,"decision",profiler)
  end
  if game.tick % 300 == 0 then grid.prune(s.grid,keep_regions(s,v)) end
  -- Re-evaluate brake/reverse mapping each tick across a speed sign change.
  vehicle_api.apply(v,s.action)
end
return M
