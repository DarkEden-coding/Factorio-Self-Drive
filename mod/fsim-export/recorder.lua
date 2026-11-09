local scenarios = require("scenarios")
local normalize = require("normalize")
local M = {}
local function write(job, data, append)
  normalize.write_file(job.path, data, append, job.player)
end
local function sample(job, inputs)
  local vehicle = job.vehicle
  local state = job.last_state
  if vehicle.valid then
    state = {x = vehicle.position.x, y = vehicle.position.y, orientation = vehicle.orientation,
      speed = vehicle.speed, health = vehicle.health or 0}
  else
    state = {x = state.x, y = state.y, orientation = state.orientation, speed = 0, health = 0}
  end
  job.last_state = state
  local health = {}
  for i, entity in ipairs(job.obstacles) do health[i] = entity.valid and (entity.health or 0) or 0 end
  local row = {tick = job.elapsed, inputs = inputs, x = state.x, y = state.y,
    orientation = state.orientation, speed = state.speed, health = state.health, obstacle_health = health}
  write(job, (job.elapsed == 0 and "" or ",") .. normalize.json(row), true)
end
local function start_next(queue, player)
  if #queue == 0 then storage.fsim_record = nil; game.print("fsim: recording queue complete"); return end
  local index = table.remove(queue, 1)
  local scenario = scenarios[index]
  local force = game.forces["fsim-calibration"] or game.create_force("fsim-calibration")
  local surface = game.create_surface("fsim-calibration-" .. game.tick, {
    width = scenario.surface.area[2][1] - scenario.surface.area[1][1],
    height = scenario.surface.area[2][2] - scenario.surface.area[1][2], seed = 1,
    peaceful_mode = true, no_enemies_mode = true, default_enable_all_autoplace_controls = false,
    autoplace_settings = {entity = {treat_missing_as_default = false, settings = {}},
      decorative = {treat_missing_as_default = false, settings = {}},
      tile = {treat_missing_as_default = false, settings = {}}},
    cliff_settings = {name = "cliff", control = "nauvis_cliff", cliff_elevation_0 = 1024,
      cliff_elevation_interval = 40, cliff_smoothing = 0, richness = 0}})
  surface.always_day, surface.daytime, surface.peaceful_mode = true, 0.5, true
  surface.freeze_daytime = true
  for name, value in pairs(scenario.surface.properties or {}) do surface.set_property(name, value) end
  storage.fsim_record = {queue = queue, player = player, scenario = scenario, force = force,
    surface = surface, cx = scenario.surface.area[1][1] / 32, cy = scenario.surface.area[1][2] / 32,
    elapsed = 0, path = "fsim/traces/" .. scenario.name .. ".json", obstacles = {}}
  game.print("fsim: preparing " .. scenario.name)
end
function M.start(name, player, filters)
  assert(not storage.fsim_record, "A recording is already running")
  local queue = {}
  for i, scenario in ipairs(scenarios) do
    local matches = not filters or #filters == 0
    for _, filter in ipairs(filters or {}) do
      if scenario.name:find(filter, 1, true) then matches = true end
    end
    if (name == nil or scenario.name == name) and matches then queue[#queue + 1] = i end
  end
  if #queue == 0 then error("Unknown scenario; see scenarios.lua or /fsim-list-scenarios") end
  start_next(queue, player)
end
function M.list(player)
  for _, scenario in ipairs(scenarios) do player.print(scenario.name) end
end
local function prepare_chunk(job)
  local s = job.scenario.surface
  local x0, y0 = job.cx * 32, job.cy * 32
  job.surface.request_to_generate_chunks({x0 + 16, y0 + 16}, 0)
  job.surface.force_generate_chunk_requests()
  for _, entity in pairs(job.surface.find_entities_filtered({area = {{x0, y0}, {x0 + 32, y0 + 32}}})) do
    entity.destroy()
  end
  job.surface.destroy_decoratives({area = {{x0, y0}, {x0 + 32, y0 + 32}}})
  local tiles = {}
  for y = y0, y0 + 31 do
    for x = x0, x0 + 31 do
      local name = s.tile
      for _, patch in ipairs(s.water) do
        if x >= patch.area[1][1] and x < patch.area[2][1] and
          y >= patch.area[1][2] and y < patch.area[2][2] then name = patch.name end
      end
      tiles[#tiles + 1] = {name = name, position = {x, y}}
    end
  end
  job.surface.set_tiles(tiles, false)
  job.prepared_chunks = (job.prepared_chunks or 0) + 1
  job.cx = job.cx + 1
  if job.cx * 32 >= s.area[2][1] then job.cx, job.cy = s.area[1][1] / 32, job.cy + 1 end
  return job.cy * 32 >= s.area[2][2]
end
local function spawn(job)
  local s = job.scenario
  for i, obstacle in ipairs(s.obstacles) do
    local e = assert(job.surface.create_entity({name = obstacle.name, position = obstacle.position,
      cliff_orientation = obstacle.cliff_orientation, force = "neutral", snap_to_grid = false}),
      "Could not create " .. obstacle.name)
    job.obstacles[i] = e
    obstacle.position = normalize.position(e.position)
    if e.type == "cliff" then obstacle.cliff_orientation = e.cliff_orientation end
    obstacle.health = e.health
  end
  local v = s.vehicle
  job.vehicle = assert(job.surface.create_entity({name = v.name, position = v.position,
    force = job.force, orientation = v.orientation, quality = "normal"}), "Could not create vehicle")
  job.driver = assert(job.surface.create_entity({name = "character", position = {10, 0},
    force = job.force}), "Could not create driver")
  job.vehicle.set_driver(job.driver)
  assert(job.vehicle.get_driver() == job.driver, "Standalone character was not seated")
  assert(not job.vehicle.grid or #job.vehicle.grid.equipment == 0, "Unexpected vehicle equipment")
  for _, technology in pairs(job.force.technologies) do
    assert(not technology.researched, "Calibration force has researched technologies")
  end
  local inventory = assert(job.vehicle.get_fuel_inventory(), "Vehicle has no fuel inventory")
  assert(inventory.insert({name = s.fuel, count = prototypes.item[s.fuel].stack_size}) > 0)
  job.vehicle.speed = v.speed
  v.position, v.orientation, v.health = normalize.position(job.vehicle.position),
    job.vehicle.orientation, job.vehicle.health
  job.last_state = {x = v.position[1], y = v.position[2], orientation = v.orientation,
    speed = v.speed, health = v.health}
  local header = normalize.json({schema_version = 1, header = {name = s.name,
    source = normalize.source(), surface = s.surface, vehicle = v, fuel = s.fuel,
    obstacles = s.obstacles, inputs = s.inputs, preparation = {chunks = job.prepared_chunks,
      standalone_driver = true, unresearched_force = true, empty_equipment = true},
    sample_timing = "post-step; tick 0 is initial state"}})
  write(job, header:sub(1, -2) .. ',"samples":[', false)
  sample(job, {throttle = 0, steer = 0})
  job.segment, job.remaining = 1, s.inputs[1].ticks
end
local function apply(job)
  local input = job.scenario.inputs[job.segment]
  local acceleration = "nothing"
  local speed = job.vehicle.valid and job.vehicle.speed or 0
  if input.throttle == 1 then acceleration = speed < 0 and "braking" or "accelerating" end
  if input.throttle == -1 then acceleration = speed > 0 and "braking" or "reversing" end
  local direction = input.steer == -1 and "left" or (input.steer == 1 and "right" or "straight")
  if job.vehicle.valid then
    job.vehicle.riding_state = {acceleration = defines.riding.acceleration[acceleration],
      direction = defines.riding.direction[direction]}
    local inventory = job.vehicle.get_fuel_inventory()
    inventory.insert({name = job.scenario.fuel, count = 1})
  end
  job.applied = {throttle = input.throttle, steer = input.steer, acceleration = acceleration}
end
function M.tick()
  local job = storage.fsim_record
  if not job then return end
  if not job.vehicle then
    if not prepare_chunk(job) then return end
    spawn(job)
    apply(job)
    return
  end
  job.elapsed = job.elapsed + 1
  sample(job, job.applied)
  job.remaining = job.remaining - 1
  if not job.vehicle.valid then
    write(job, '],"ended":"vehicle-destroyed","death_position_captured":' ..
      tostring(job.death_position_captured or false) .. '}', true)
  elseif job.remaining == 0 and job.segment == #job.scenario.inputs then
    write(job, '],"ended":"complete"}', true)
  else
    if job.remaining == 0 then
      job.segment = job.segment + 1
      job.remaining = job.scenario.inputs[job.segment].ticks
    end
    apply(job)
    return
  end
  game.print("fsim: trace complete: script-output/" .. job.path)
  storage.fsim_completed = storage.fsim_completed or {}
  storage.fsim_completed[#storage.fsim_completed + 1] = job.scenario.name
  -- 2.1.24 crashes deleting a surface after set_property; such surfaces are left in place.
  if not job.scenario.surface.properties then game.delete_surface(job.surface) end
  storage.fsim_record = {queue = job.queue, player = job.player, next_pending = true}
end
function M.on_died(event)
  local job = storage.fsim_record
  if job and job.vehicle and event.entity == job.vehicle then
    local entity = event.entity
    job.last_state = {x = entity.position.x, y = entity.position.y,
      orientation = entity.orientation, speed = 0, health = 0}
    job.death_position_captured = true
  end
end
function M.abort(message)
  storage.fsim_error = tostring(message)
  local job = storage.fsim_record
  storage.fsim_record = nil
  if job and job.surface and job.surface.valid then game.delete_surface(job.surface) end
  game.print("fsim: recording aborted: " .. tostring(message) .. ". Discard any unfinished trace file.")
end
function M.advance()
  local job = storage.fsim_record
  if job and job.next_pending then start_next(job.queue, job.player) else M.tick() end
end
return M
