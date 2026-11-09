-- Rolling, cached any-angle corridor with deterministic background work budgets.
local field = require("field")
local grid = require("grid")
local corridor = require("corridor")
local raster = require("raster")
local observation = require("observation")
local network = require("network")
local vehicle_api = require("vehicle")
local trains = require("trains")
local u = require("math-util")
local escape = require("escape")
local M = {}
-- Cars on Vulcanus coast instead of accelerating above this speed (km/h): its open plains
-- let fast fuel reach 180+ km/h, where the model's 32-tile view is too short to dodge rocks
-- (docs/vulcanus-long-test.md). scripts/autodrive-sim.mjs applies the same limit.
local VULCANUS_CAR_LIMIT = 130
local function speed_limit(vehicle)
  local planet = vehicle.surface.planet
  if vehicle.name == "car" and planet and planet.name == "vulcanus" then return VULCANUS_CAR_LIMIT end
end
-- Forward throttle above the limit becomes coasting with the same steering.
local function governed(s,v,action)
  if s.speed_limit and v.speed*216 > s.speed_limit and math.floor(action/3) == 2 then return 3 + action%3 end
  return action
end
local field_options = {}
for k,v in pairs(network.model.field or {}) do field_options[k]=v end
field_options.smooth = network.model.route_source == "field_smooth"
-- With "Avoid train tracks": each tile near a rail costs this many extra tiles
-- of travel, so a crossing (about six such tiles) is worth a detour of ~50.
local RAIL_COST = 8
local rail_field_options = {rail_cost=RAIL_COST}
for k,v in pairs(field_options) do rail_field_options[k]=v end

local FIRST_SEGMENT = 48     -- tiles to the first sub-target
local SEGMENT = 160          -- tiles to each later sub-target
local FINAL_WITHIN = 200     -- the final target becomes the sub-target when this close
local SINGLE_WITHIN = 64     -- targets this close are planned as one segment, like training
local MARGIN = 32            -- field margin around a segment
local FINAL_MARGIN = 48      -- the simulator's margin, for single-segment trips
local MAX_MARGIN = 128       -- a route clipped by its window replans with a doubled margin up to this
local HANDOFF = 50           -- overlap includes 20 extra tiles beyond the old handoff span
local FIELD_BUDGET = 4000    -- field work units per tick (~1-2 ms)
local GRID_BLOCKS = 4        -- 16x16 cache blocks built per tick (forest blocks are costly)
local RECOUNT_TICKS = 60     -- how often blocks around a driving vehicle are recounted
local HORIZON = 768          -- soft horizon; one section may extend past it
local MAX_SECTIONS = 6
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
  local options = {terrain_friction_modifier=stats.terrain_friction_modifier}
  for k,v in pairs(s.avoid_rails and rail_field_options or field_options) do options[k]=v end
  local f = field.new(from,target,stats.half_width,stats.tank_driving,nil,margin,options)
  grid.ensure(s.grid,f.x0-3,f.y0-3,f.x1+3,f.y1+3,false)
  return {from=from,target=target,aim=target,final=final,margin=margin,field=f,phase="grid",attempt=0}
end
-- Within EDGE tiles of the window counts as along it: a vehicle cutting a corner there at
-- speed leaves the covered region (two tiles was not enough for a tank in a cliff maze).
local EDGE = 6
local function clipped(f,route)
  for _,p in ipairs(route) do
    if p.x<f.x0+EDGE or p.y<f.y0+EDGE or p.x>f.x1-EDGE or p.y>f.y1-EDGE then return true end
  end
  return false
end
-- Advances a plan by one tick of work. Returns "ready", "failed" or nil.
local function step_plan(s,p,probe)
  if p.phase == "grid" then
    if grid.background_pending(s.grid) or grid.urgent_pending(s.grid) then return end
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
    if not field.slice(p.field,function(x,y) return raster.mask(s.grid,x,y) end,FIELD_BUDGET,
      function(x,y) return raster.friction(s.grid,x,y) end) then return end
    if field.sample(p.field,probe.x,probe.y) == math.huge then p.phase = "unreachable"; return "failed" end
    -- A route along the window edge means the window cut off the way around (cliff mazes):
    -- the vehicle then leaves the covered region. Plan again with a doubled margin.
    if p.margin < MAX_MARGIN and clipped(p.field,field.route(p.field,probe)) then
      local q = new_plan(s,p.from,p.target,p.final,math.min(MAX_MARGIN,p.margin*2))
      p.field, p.margin, p.phase = q.field, q.margin, q.phase
      return
    end
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

-- The trip's time limit follows training (600 ticks plus 12 per tile of route), with the
-- route length estimated as the longest of the straight line, the first field's distance
-- (exact for single-section trips) and the pathfinder's path. Straight lines alone timed out
-- winding cliff routes. Only ever raised.
local function raise_time_limit(s,distance)
  local limit=math.max(1,math.ceil(600+12*distance))
  if not s.time_limit or limit>s.time_limit then s.time_limit=limit end
end
local function path_length(path)
  local length=0
  for i=2,#path do length=length+u.distance(path[i-1],path[i]) end
  return length
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
    if s.time_limit then raise_time_limit(s,path_length(s.path)) end
  elseif event.try_again_later and (s.path_retries or 0) < PATH_RETRIES then
    s.path_retries = (s.path_retries or 0) + 1
    s.path_retry_at = game.tick + 60
  else
    s.path_failed = true
  end
end

-- options.avoid_rails: plan routes around train tracks where possible.
function M.start(vehicle,target,options)
  local avoid_rails = options and options.avoid_rails or false
  assert(vehicle.valid and (vehicle.name=="car" or vehicle.name=="tank"),"Enter a base car or tank first")
  assert(u.distance(vehicle.position,target)<=MAX_TARGET,"Target must be within "..MAX_TARGET.." tiles")
  assert(generated(vehicle.surface,target),"Target must be in an explored (generated) chunk")
  local s = {vehicle=vehicle,target={x=target.x,y=target.y},grid=grid.new({rails=avoid_rails}),avoid_rails=avoid_rails,status="planning",
    elapsed=0,stall=0,best_tick=0,decisions=0,slices=0,planning_ticks=0,action=4,route={},segments=0,
    stats=vehicle_api.stats(vehicle),single=u.distance(vehicle.position,target)<=SINGLE_WITHIN,
    corridor={},refinements=0,max_horizon=0,frontier_wait_ticks=0,yield_ticks=0,
    speed_limit=speed_limit(vehicle)}
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

-- Handoffs change field coverage, never the trip's destination or clock.
local function activate(s,p)
  s.field, s.seg_target, s.final_segment = p.field, p.target, p.final
  s.best_distance=field.sample(p.field,s.vehicle.position.x,s.vehicle.position.y)
  if not s.time_limit then
    local estimate=u.distance(s.vehicle.position,s.target)
    if s.best_distance<math.huge then
      estimate=math.max(estimate,s.best_distance+(p.final and 0 or u.distance(p.target,s.target)))
    end
    if s.path then estimate=math.max(estimate,path_length(s.path)) end
    raise_time_limit(s,estimate)
  end
  s.segments = s.segments + 1
  s.plan=nil
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
  if s.profile then helpers.write_file("factorio-autodrive/timing.txt",{"",kind," ",profiler,"\n"},true) end
end
local function route_at(s)
  local route,cursor=corridor.trim(s.guidance,s.vehicle.position,s.guidance_cursor)
  if route then s.guidance_cursor=cursor; return route end
  -- Recover from a real deviation using verified fields. Do not feed the
  -- vehicle-to-route correction back into the cached polyline every decision.
  s.guidance=corridor.refine(s.corridor,s.vehicle.position)
  s.guidance_cursor=2
  return s.guidance
end
function M.observe(s)
  if s.single then s.route=field.route(s.field,s.vehicle.position)
  else s.route=route_at(s) end
  local state = M.snapshot(s)
  s.stats = vehicle_api.stats(s.vehicle)
  local obs,look = observation.build(s.grid,s.route,state,s.stats,trains.overlay(s.vehicle.surface,s.vehicle.position))
  local action,logits = network.forward(obs)
  s.look = look
  return action,{state=state,stats=s.stats,observation=obs,logits=logits,action=action,route=s.route,
    spec_hash=network.model.spec_hash,segment=s.segments,final_segment=s.final_segment,
    route_continues=not s.corridor[#s.corridor].final,verified_horizon=u.length(s.route),
    destination=s.target}
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

local function publish(s)
  local previous=corridor.trim(s.guidance,s.vehicle.position,s.guidance_cursor)
  s.guidance=corridor.refine(s.corridor,s.vehicle.position,previous)
  s.guidance_cursor=2
  s.route=s.guidance
  s.max_horizon=math.max(s.max_horizon,u.length(s.guidance))
  s.refinements=s.refinements+1
  s.max_cached_sections=math.max(s.max_cached_sections or 0,#s.corridor)
end
-- One worker, one field slice per tick. Reuse every solved section until it is
-- consumed, extending multiple sections ahead instead of waiting for a handoff.
local function plan_ahead(s,v)
  if s.repair_index then
    local i=s.repair_index
    local old=s.corridor[i]
    if not s.repair then
      local from=i==1 and {x=v.position.x,y=v.position.y} or old.from
      s.repair=new_plan(s,from,old.target,old.final,old.margin)
    end
    local result=step_plan(s,s.repair,s.repair.from)
    s.slices=s.slices+1
    if result=="ready" then
      s.corridor[i]=s.repair
      for j=#s.corridor,i+1,-1 do s.corridor[j]=nil end
      if i==1 then
        s.field,s.seg_target,s.final_segment=s.repair.field,s.repair.target,s.repair.final
        s.best_distance=field.sample(s.field,v.position.x,v.position.y)
      end
      s.path_cursor=nil
      s.repair,s.repair_index,s.extension=nil,nil,nil
      publish(s)
    elseif result=="failed" then
      s.repair=retry(s,s.repair)
      if not s.repair then return "unreachable" end
    end
    return
  end
  local last=s.corridor[#s.corridor]
  if last.final then return end
  if not s.extension then
    local route=route_at(s)
    if #s.corridor>=MAX_SECTIONS or (route and u.length(route)>=HORIZON) then return end
    local tail=field.route(last.field,last.from)
    local from=u.point_along(tail,math.max(0,u.length(tail)-HANDOFF-20))
    local target,final=pick(s,last.target,SEGMENT)
    s.extension=new_plan(s,from,target,final,final and FINAL_MARGIN or MARGIN)
  end
  local p=s.extension
  if p.phase=="exhausted" then return end
  local result=step_plan(s,p,p.from)
  s.slices=s.slices+1
  if result=="ready" then
    -- Both endpoints must connect; never splice an unverified gap.
    if field.sample(p.field,last.target.x,last.target.y)==math.huge then
      s.extension=retry(s,p) or {phase="exhausted"}
      return
    end
    s.corridor[#s.corridor+1]=p
    s.extension=nil
    publish(s)
  elseif result=="failed" then s.extension=retry(s,p) or {phase="exhausted"} end
end

-- Removals leave the old route conservative. Rebuild affected coverage in the
-- background and replace dependent sections only after its replacement is ready.
function M.dirty(s,position)
  grid.dirty(s.grid,position)
  local function affected(f)
    return f and position.x>=f.x0-32 and position.x<f.x1+32 and
      position.y>=f.y0-32 and position.y<f.y1+32
  end
  for i,p in ipairs(s.corridor or {}) do
    if affected(p.field) then
      s.repair_index=math.min(s.repair_index or i,i)
      s.repair=nil
      break
    end
  end
  for _,key in ipairs({"plan","extension"}) do
    local p=s[key]
    if p and affected(p.field) then s[key]=new_plan(s,p.from,p.target,p.final,p.margin) end
  end
end
-- Parked-train changes are batched: applying each at once would restart route
-- planning over and over in a busy rail network. Changes near the vehicle are
-- also caught by the recount in M.tick within a second.
local TRAIN_DIRTY_TICKS = 300
function M.train_dirty(s,position)
  s.train_dirty = s.train_dirty or {}
  s.train_dirty[math.floor(position.x/16)..":"..math.floor(position.y/16)] = {x=position.x,y=position.y}
end
local function flush_train_dirty(s)
  if not s.train_dirty or game.tick < (s.train_dirty_at or 0) then return end
  for _, position in pairs(s.train_dirty) do M.dirty(s,position) end
  s.train_dirty, s.train_dirty_at = nil, game.tick + TRAIN_DIRTY_TICKS
end
local function keep_regions(s,v)
  local keep={{x0=v.position.x-48,y0=v.position.y-48,x1=v.position.x+49,y1=v.position.y+49}}
  local function add(f)
    if f then keep[#keep+1]={x0=f.x0-3,y0=f.y0-3,x1=f.x1+3,y1=f.y1+3} end
  end
  for _,p in ipairs(s.corridor) do add(p.field) end
  for _,key in ipairs({"plan","extension","repair"}) do if s[key] then add(s[key].field) end end
  return keep
end

function M.tick(s)
  local v = s.vehicle
  if not v.valid then s.status="destroyed"; return "destroyed" end
  if s.status == "paused" then return end
  if s.status ~= "planning" and s.status ~= "driving" then vehicle_api.brake(v); return end
  local profiler = helpers.create_profiler()
  if s.status=="driving" then
    local x, y = math.floor(v.position.x), math.floor(v.position.y)
    grid.ensure(s.grid,x-36,y-36,x+37,y+37,true)
    -- Buildings another mod placed or removed without an event.
    if game.tick % RECOUNT_TICKS == 0 then
      for _, centre in ipairs(grid.changed(s.grid,v.surface,x-36,y-36,x+37,y+37)) do M.dirty(s,centre) end
    end
  end
  flush_train_dirty(s)
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
    if result == "ready" then
      activate(s,p)
      s.corridor={p}
      publish(s)
      s.status,s.previous_action = "driving",nil
    elseif result == "failed" then
      local q = retry(s,p)
      if q then s.plan, s.field = q, q.field
      elseif (s.path_request or s.path) and not s.retried_on_path then s.plan = nil
      else return finish(s,"unreachable") end
    end
    if s.planning_ticks>=3600 then return finish(s,"planning-timeout") end
    profile(s,"route",profiler)
    return
  end
  -- Engine-tick arrival check at the destination (s.target is always the final target,
  -- whichever section is active).
  if u.distance(v.position,s.target)<=1 and math.abs(v.speed)<=0.01 then
    return finish(s,"arrived")
  end
  if s.path_retry_at and game.tick >= s.path_retry_at then s.path_retry_at = nil; request_path(s) end
  local failure=plan_ahead(s,v)
  if failure then return finish(s,failure) end
  if s.segments>s.max_segments then return finish(s,"unreachable (too many detours)") end
  if #s.corridor>1 and not s.repair_index and game.tick%16==0 then
    local route=route_at(s)
    -- A union shortcut can skip several section windows. Select the farthest
    -- covering field whose remaining corridor still verifies the entire route.
    -- The final section needs no route check: its field reaches the destination by
    -- itself, and a destination beside an obstacle (inside its clearance) never passes
    -- the check, which used to leave stall tracking on an earlier section's field.
    for i=#s.corridor,2,-1 do
      local next=s.corridor[i]
      local f=next.field
      if v.position.x>f.x0+4 and v.position.x<f.x1-4 and
        v.position.y>f.y0+4 and v.position.y<f.y1-4 and
        field.sample(f,v.position.x,v.position.y)<math.huge and
        (next.final or corridor.covered(s.corridor,route,i)) then
        for _=1,i-1 do table.remove(s.corridor,1) end
        activate(s,next)
        break
      end
    end
  end
  if not s.single and #s.corridor>1 and game.tick%120==0 then publish(s) end
  -- Waiting for a train does not count against the trip's clock or stall limit.
  s.elapsed=s.elapsed+((s.started and not s.yielding) and 1 or 0)
  s.started=true
  local distance=field.sample(s.field,v.position.x,v.position.y)
  if distance<math.huge and distance<s.best_distance-1 then s.best_distance,s.best_tick=distance,s.elapsed end
  s.stall=s.elapsed-s.best_tick
  -- Stop safely at the verified frontier if extension has not caught up.
  -- The fixed destination stays visible to the policy; frontier braking is a
  -- planner constraint, not a temporary destination the actor learns to stop at.
  if s.elapsed>=s.time_limit then return finish(s,"timeout") end
  if s.stall>=1200 then return finish(s,"stalled") end
  local frontier=not s.corridor[#s.corridor].final
  local remaining=frontier and u.length(route_at(s)) or math.huge
  local braking=trains.stopping_distance(s.stats,v.speed)
  if frontier and remaining<math.max(12,braking+8) then
    if s.extension and s.extension.phase=="exhausted" and math.abs(v.speed)<=0.01 then return finish(s,"unreachable") end
    s.frontier_wait_ticks=s.frontier_wait_ticks+1
    vehicle_api.brake(v)
    profile(s,"frontier",profiler)
    return
  end
  -- Rays need cached blocks around the vehicle. Wait only for those.
  if grid.urgent_pending(s.grid) then
    vehicle_api.brake(v)
    profile(s,"cache",profiler)
    return
  end
  local covered=false
  for _,p in ipairs(s.corridor) do
    local f=p.field
    if v.position.x>=f.x0+1 and v.position.x<f.x1-1 and
      v.position.y>=f.y0+1 and v.position.y<f.y1-1 then covered=true; break end
  end
  if not covered then return finish(s,"outside-route-region") end
  -- The trip clock stops while holding for a train, so recheck on the game clock.
  local decision=s.yielding and game.tick%4==0 or (not s.yielding and (s.elapsed-1)%4==0)
  if decision then
    local action=M.observe(s)
    s.yielding=trains.hazard(v,s.stats,s.route)
    if s.yielding then
      -- Like planning: the model resumes without a previous action.
      s.previous_action,s.escape=nil,nil
    else
      s.escape=s.escape or escape.new()
      action,s.escaping=escape.decide(s.escape,v.position,action)
      action=governed(s,v,action)
      s.action,s.previous_action,s.decisions=action,action,s.decisions+1
    end
  end
  if s.yielding then
    s.yield_ticks=s.yield_ticks+1
    vehicle_api.brake(v)
    profile(s,"train",profiler)
    return
  end
  if game.tick % 300 == 0 then grid.prune(s.grid,keep_regions(s,v)) end
  -- Re-evaluate brake/reverse mapping each tick across a speed sign change.
  vehicle_api.apply(v,s.action)
  profile(s,decision and "decision" or "drive",profiler)
end
return M
