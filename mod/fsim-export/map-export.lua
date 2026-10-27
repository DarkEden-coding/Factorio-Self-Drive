local normalize = require("normalize")
local M = {}
local function start(surface, radius, cx, cy, player, name, generate)
  assert(not storage.fsim_map, "A map export is already running")
  assert(radius == math.floor(radius) and radius >= 0 and radius <= 31, "radius must be in 0..31")
  local job = {surface = surface, player = player, radius = radius, cx = cx, cy = cy,
    generate = generate, generated_chunks = 0, exported_chunks = 0,
    dx = -radius, dy = -radius, first = true, path = "fsim/maps/" .. name .. ".json"}
  local header = normalize.json({schema_version = 1, source = normalize.source(),
    surface = surface.name, area = {{(cx - radius) * 32, (cy - radius) * 32},
      {(cx + radius + 1) * 32, (cy + radius + 1) * 32}},
    seed = surface.map_gen_settings.seed, started_tick = game.tick})
  normalize.write_file(job.path, header:sub(1, -2) .. ',"chunks":[', false, player)
  storage.fsim_map = job
end
function M.start(player, radius)
  local name = player.surface.name:gsub("[^%w_-]", "_") .. "-" .. game.tick .. "-p" .. player.index
  start(player.surface, radius, math.floor(player.position.x / 32),
    math.floor(player.position.y / 32), player.index, name, false)
  player.print("fsim: exporting to script-output/" .. storage.fsim_map.path)
end
function M.start_generated(surface, radius)
  start(surface, radius, 0, 0, false, "nauvis-seed-" .. surface.map_gen_settings.seed, true)
end
local function chunk(surface, cx, cy)
  local area = {{cx * 32, cy * 32}, {(cx + 1) * 32, (cy + 1) * 32}}
  local result = {position = {cx, cy}, tiles = {}, resources = {}, entities = {}}
  for _, tile in pairs(surface.find_tiles_filtered({area = area})) do
    local i = (tile.position.y - cy * 32) * 32 + tile.position.x - cx * 32 + 1
    result.tiles[i] = tile.name
  end
  for _, entity in pairs(surface.find_entities_filtered({area = area,
    type = {"resource", "tree", "simple-entity", "cliff", "unit-spawner"}})) do
    local p = entity.position
    -- Area queries include overlapping bounding boxes. Own each entity by its center chunk.
    if math.floor(p.x / 32) == cx and math.floor(p.y / 32) == cy and
      (entity.type ~= "simple-entity" or entity.prototype.count_as_rock_for_filtered_deconstruction) then
      if entity.type == "resource" then
        result.resources[#result.resources + 1] = {name = entity.name,
          position = {math.floor(p.x), math.floor(p.y)}}
      else
        result.entities[#result.entities + 1] = {name = entity.name, kind = normalize.kind(entity.type),
          position = normalize.position(p), orientation = entity.orientation, health = entity.health,
          cliff_orientation = entity.type == "cliff" and entity.cliff_orientation or nil}
      end
    end
  end
  return result
end
function M.tick()
  local job = storage.fsim_map
  if not job then return end
  if job.surface.valid then
    local cx, cy = job.cx + job.dx, job.cy + job.dy
    if job.generate and not job.surface.is_chunk_generated({cx, cy}) then
      job.surface.request_to_generate_chunks({cx * 32 + 16, cy * 32 + 16}, 0)
      job.surface.force_generate_chunk_requests()
      job.generated_chunks = job.generated_chunks + 1
    end
    if job.surface.is_chunk_generated({cx, cy}) then
      local json = normalize.json(chunk(job.surface, cx, cy))
      normalize.write_file(job.path, (job.first and "" or ",") .. json, true, job.player)
      job.first = false
      job.exported_chunks = (job.exported_chunks or 0) + 1
    end
    job.dx = job.dx + 1
    if job.dx > job.radius then job.dx, job.dy = -job.radius, job.dy + 1 end
    if job.dy <= job.radius then return end
  end
  normalize.write_file(job.path, '],"finished_tick":' .. game.tick ..
    ',"generated_chunks":' .. (job.generated_chunks or 0) ..
    ',"exported_chunks":' .. (job.exported_chunks or 0) .. '}', true, job.player)
  game.print("fsim: map complete: script-output/" .. job.path)
  storage.fsim_map = nil
end
return M
