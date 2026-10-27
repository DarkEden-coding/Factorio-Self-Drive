local options = require("config")
local normalize = require("__fsim-export__.normalize")
local function fail(message)
  storage.finished = true
  helpers.write_file("fsim/error.json", normalize.json({error = tostring(message),
    tick = game.tick}), false)
end
script.on_init(function() storage.pending = true end)
script.on_event(defines.events.on_tick, function()
  if storage.finished then return end
  local ok, message = pcall(function()
    if storage.pending then
      storage.pending = nil
      storage.started_tick = game.tick
      helpers.write_file("fsim/graphics.json", normalize.json(prototypes.mod_data["fsim-graphics"].data), false)
      remote.call("fsim-export", "start", options)
    end
    local status = remote.call("fsim-export", "status")
    if status.error then error(status.error) end
    if status.recording or status.mapping then return end
    helpers.write_file("fsim/done.json", normalize.json({schema_version = 1,
      started_tick = storage.started_tick, finished_tick = game.tick,
      game_version = helpers.game_version, active_mods = script.active_mods,
      options = options, completed = status.completed, players = #game.players}), false)
    storage.finished = true
  end)
  if not ok then fail(message) end
end)
