-- Train awareness. The driving model was never trained with trains, so:
-- parked trains become hard obstacles for route planning (grid.lua);
-- every carriage near the vehicle is shown to the model as a hard obstacle
-- (M.overlay); and a rule-based check (M.hazard) holds the vehicle before a
-- level crossing when a train could reach it before the vehicle is across.
local raster = require("raster")
local u = require("math-util")
local M = {}

M.ROLLING_STOCK = {"locomotive","cargo-wagon","fluid-wagon","artillery-wagon","infinity-cargo-wagon"}
-- Ground-level rails. Vehicles pass under elevated rails and their trains.
-- Ramps count, since their low end is at ground level.
local GROUND_RAILS = {"straight-rail","half-diagonal-rail","curved-rail-a","curved-rail-b",
  "legacy-straight-rail","legacy-curved-rail","rail-ramp"}
M.GROUND_RAILS = GROUND_RAILS
-- States in which a stopped train stays put until something changes its state
-- (raising on_train_changed_state), so it can be planned around like a building.
-- A stopped train in arrive_station is standing at its stop.
local PARKED_STATES = {
  [defines.train_state.arrive_station]=true,[defines.train_state.wait_station]=true,[defines.train_state.manual_control]=true,
  [defines.train_state.manual_control_stop]=true,[defines.train_state.no_path]=true,
  [defines.train_state.no_schedule]=true,[defines.train_state.destination_full]=true}
M.PARKED_STATES = PARKED_STATES

local CARRIAGE_HALF_WIDTH = 0.6  -- base rolling stock collision box half width
local MARGIN = 1                 -- extra clearance between vehicle and train, tiles
local SAMPLE = 0.5               -- route sampling step, tiles
-- A loaded single locomotive gains about 0.004 tiles/tick each tick from rest;
-- assume more, for better fuel and fewer wagons.
local TRAIN_ACCELERATION = 0.006
local OVERLAY_RADIUS = 40

local function elevated(rail) return rail.type:sub(1,9) == "elevated-" end
function M.on_ground(train)
  return not (elevated(train.front_end.rail) and elevated(train.back_end.rail))
end
function M.parked(train)
  return train.speed == 0 and PARKED_STATES[train.state] == true
end
-- For on_train_changed_state: did the train start or stop counting as parked?
-- `known` maps train id to the last parked status seen (kept in storage).
-- Trains at a busy stop cycle through parked states many times a second;
-- those changes leave the map as it was.
function M.parked_changed(train,known)
  local parked = M.parked(train)
  local changed = known[train.id] ~= parked
  known[train.id] = parked
  return changed
end
-- Carriage as a raster entity: its oriented collision box, lengthened over the
-- couplings so a train has no gaps a vehicle could try to squeeze through.
local COUPLING = 1.2
function M.shape(carriage)
  local box = carriage.prototype.collision_box
  return {x=carriage.position.x,y=carriage.position.y,kind="train",orientation=carriage.orientation,
    health=carriage.health,box={{box.left_top.x,box.left_top.y-COUPLING},{box.right_bottom.x,box.right_bottom.y+COUPLING}}}
end

-- Rail as a raster entity: a 2-tile-wide box along the chord between its ends
-- (curves bulge less than half a tile from it).
function M.rail_shape(rail)
  local a = rail.get_rail_end(defines.rail_direction.front).location.position
  local b = rail.get_rail_end(defines.rail_direction.back).location.position
  local half = u.distance(a,b)/2
  return {x=(a.x+b.x)/2,y=(a.y+b.y)/2,kind="rail",
    orientation=math.atan2(b.x-a.x,a.y-b.y)/(2*math.pi),box={{-1,-half},{1,half}}}
end

-- Tiles covered by ground-level carriages within OVERLAY_RADIUS of position,
-- as overlay[y][x] = mask bits, or nil when there are none.
function M.overlay(surface,position)
  local r = OVERLAY_RADIUS
  local x0, y0 = math.floor(position.x-r), math.floor(position.y-r)
  local x1, y1 = x0+2*r, y0+2*r
  local overlay
  for _, carriage in pairs(surface.find_entities_filtered({
    area={{x0-4,y0-4},{x1+5,y1+5}},type=M.ROLLING_STOCK})) do
    if M.on_ground(carriage.train) then
      overlay = overlay or {}
      raster.stamp(M.shape(carriage),x0,y0,x1,y1,function(x,y)
        local row = overlay[y] or {}
        overlay[y] = row
        row[x] = raster.bits.train
      end)
    end
  end
  return overlay
end

-- Distance a vehicle needs to brake to a stop. Braking removes a fixed amount
-- of kinetic energy per tick; integrating v(t) gives m*v^3/(3*power).
function M.stopping_distance(stats,speed)
  return stats.weight*(math.abs(speed)*60)^3/(3*math.max(stats.braking_power,1))
end
-- Ticks for the vehicle to cover distance at full throttle from speed, using
-- the same energy model (friction ignored, capped at top speed).
local function ticks_to_cover(stats,speed,distance)
  local v, covered, gain = math.max(speed,0), 0, 2*stats.power/(60*stats.weight*3600)
  for tick = 1, 1200 do
    v = math.min(stats.top_speed,math.sqrt(v*v+gain))
    covered = covered + v
    if covered >= distance then return tick end
  end
  return 1200
end
-- Distance a train can cover in `ticks`, accelerating as fast as any train might.
local function reach(train,ticks)
  local v = math.abs(train.speed)
  local top = math.max(v,train.speed < 0 and train.max_backward_speed or train.max_forward_speed)
  local ramp = (top-v)/TRAIN_ACCELERATION
  if ticks <= ramp then return v*ticks+TRAIN_ACCELERATION*ticks*ticks/2 end
  return (v+top)/2*ramp+top*(ticks-ramp)
end

local function segment_distance(p,a,b)
  local dx, dy = b.x-a.x, b.y-a.y
  local len2 = dx*dx+dy*dy
  local t = len2 > 0 and u.clamp(((p.x-a.x)*dx+(p.y-a.y)*dy)/len2,0,1) or 0
  return u.distance(p,{x=a.x+t*dx,y=a.y+t*dy})
end
-- Points every SAMPLE tiles along route, up to length tiles, with their distance.
local function samples(route,length)
  local out = {}
  for d = 0, math.min(length,u.length(route)), SAMPLE do
    out[#out+1] = {p=u.point_along(route,d),d=d}
  end
  return out
end

-- Stretches of the route within reach of a ground rail: {enter, exit, at, rails}
-- with route distances, the point where it begins, and the rails' unit numbers.
-- Stretches closer together than gap tiles are merged, so the vehicle never
-- waits in a gap between tracks too narrow to be safe.
local function crossings(surface,route,length,band,gap)
  local points = samples(route,length)
  if #points == 0 then return {} end
  local x0, y0, x1, y1 = math.huge, math.huge, -math.huge, -math.huge
  for _, s in ipairs(points) do
    x0, y0 = math.min(x0,s.p.x), math.min(y0,s.p.y)
    x1, y1 = math.max(x1,s.p.x), math.max(y1,s.p.y)
  end
  -- Curved rails reach up to about 4 tiles from their centre.
  local rails = surface.find_entities_filtered({
    area={{x0-band-4,y0-band-4},{x1+band+4,y1+band+4}},type=GROUND_RAILS})
  if #rails == 0 then return {} end
  local chords = {}
  for _, rail in ipairs(rails) do
    chords[#chords+1] = {id=rail.unit_number,
      a=rail.get_rail_end(defines.rail_direction.front).location.position,
      b=rail.get_rail_end(defines.rail_direction.back).location.position}
  end
  local out, current = {}, nil
  for _, s in ipairs(points) do
    local hit
    for _, c in ipairs(chords) do
      if segment_distance(s.p,c.a,c.b) < band then
        hit = hit or {}
        hit[c.id] = true
      end
    end
    if hit then
      if current and s.d-current.exit <= gap then
        current.exit = s.d
      else
        current = {enter=s.d,exit=s.d,at=s.p,rails={}}
        out[#out+1] = current
      end
      for id in pairs(hit) do current.rails[id] = true end
    end
  end
  return out
end

-- True if the train is on one of the rails, or could reach one within ticks.
local function threatens(train,rails,at,ticks)
  local front = train.front_stock
  -- Parked trains are obstacles in the route planner's map instead.
  if not front or M.parked(train) or not M.on_ground(train) then return false end
  local limit = reach(train,ticks)
  -- Path distance is never shorter than the straight line.
  if u.distance(front.position,at) > limit+8*#train.carriages+16 then return false end
  for _, rail in ipairs(train.get_rails()) do
    if rails[rail.unit_number] then return true end
  end
  if train.speed == 0 and not train.has_path then return false end
  local previous, travelled = nil, 0
  local function passes(rail)
    if previous then travelled = travelled+u.distance(previous,rail.position) end
    previous = rail.position
    return rails[rail.unit_number]
  end
  local path = train.has_path and train.path
  if path and (train.speed == 0 or (train.speed > 0) == path.is_front) then
    for i = path.current, path.size do
      if passes(path.rails[i]) then return true end
      if travelled > limit then return false end
    end
    return false
  end
  -- Manual or reversing trains: follow the switches the way trains last went.
  local cursor = (train.speed > 0 and train.front_end or train.back_end).make_copy()
  passes(cursor.rail)
  for _ = 1, 2000 do
    if not cursor.move_natural() then return false end
    if passes(cursor.rail) then return true end
    if travelled > limit then return false end
  end
  return false
end

-- Returns "train" when the vehicle should hold before the next level crossing
-- on route, else nil. The vehicle is never held once it cannot stop before the
-- crossing, or while it is on one: then the way out is to keep driving.
function M.hazard(vehicle,stats,route)
  if not route or #route < 2 or vehicle.speed < -0.01 then return nil end
  local hw, hl = stats.half_width, stats.half_length
  local band = CARRIAGE_HALF_WIDTH+math.sqrt(hw*hw+hl*hl)+MARGIN
  local stopping = M.stopping_distance(stats,vehicle.speed)
  -- Look far enough to see the crossing's far side when deciding to hold.
  local list = crossings(vehicle.surface,route,stopping+40,band,2*hl+2)
  for _, crossing in ipairs(list) do
    if crossing.enter > 0 then
      if crossing.enter > stopping+6 or stopping >= crossing.enter then return nil end
      local ticks = math.ceil(ticks_to_cover(stats,vehicle.speed,crossing.exit+SAMPLE)*1.5)+60
      for _, train in ipairs(game.train_manager.get_trains({surface=vehicle.surface})) do
        if threatens(train,crossing.rails,crossing.at,ticks) then return "train" end
      end
      return nil
    end
  end
  return nil
end
return M
