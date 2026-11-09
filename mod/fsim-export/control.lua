local prototype_export = require("prototype-export")
local recorder = require("recorder")
local map_export = require("map-export")
local function command(name, help, handler)
  commands.add_command(name, help, function(event)
    local player = event.player_index and game.get_player(event.player_index)
    if player and not player.admin then player.print("fsim commands require admin privileges"); return end
    local ok, message = pcall(handler, event, player)
    if not ok then
      if player then player.print("fsim: " .. tostring(message)) else game.print("fsim: " .. tostring(message)) end
    end
  end)
end
command("fsim-dump-prototypes", "Write fsim/prototypes.json", function(event, player)
  prototype_export.dump(event.player_index)
  if player then player.print("fsim: wrote script-output/fsim/prototypes.json") end
end)
command("fsim-list-scenarios", "List calibration scenario names", function(_, player)
  assert(player, "Run this command as a player")
  recorder.list(player)
end)
command("fsim-record", "Record one scenario: /fsim-record <scenario>", function(event)
  assert(not storage.fsim_record, "A recording is already running")
  local name = (event.parameter or ""):match("^%s*(%S+)%s*$")
  assert(name, "Usage: /fsim-record <scenario>")
  recorder.start(name, event.player_index)
end)
command("fsim-record-all", "Record every calibration scenario in sequence", function(event)
  assert(not storage.fsim_record, "A recording is already running")
  recorder.start(nil, event.player_index)
end)
command("fsim-export-map", "Export generated chunks: /fsim-export-map <radius_chunks>", function(event, player)
  assert(player, "Run this command as a player")
  assert(not storage.fsim_map, "A map export is already running")
  local radius = tonumber(event.parameter)
  assert(radius and radius == math.floor(radius) and radius >= 0 and radius <= 31,
    "radius_chunks must be an integer in 0..31")
  map_export.start(player, radius)
end)
script.on_event(defines.events.on_tick, function()
  local ok, message = pcall(recorder.advance)
  if not ok then recorder.abort(message) end
  ok, message = pcall(map_export.tick)
  if not ok then
    storage.fsim_error = tostring(message)
    storage.fsim_map, storage.fsim_map_queue = nil, nil
    game.print("fsim: map export aborted: " .. tostring(message))
  end
end)

script.on_event(defines.events.on_entity_died, recorder.on_died)

-- Used only by the isolated fsim-autorun companion; commands above remain usable.
remote.add_interface("fsim-export", {
  start = function(options)
    assert(not storage.fsim_record and not storage.fsim_map, "Export jobs already running")
    storage.fsim_error, storage.fsim_completed = nil, {}
    prototype_export.dump(false)
    if options.traces then recorder.start(nil, false, options.filters) end
    if options.maps then map_export.start_generated(options.planets or {"nauvis"}, options.radius) end
  end,
  status = function()
    return {recording = storage.fsim_record ~= nil,
      mapping = storage.fsim_map ~= nil or storage.fsim_map_queue ~= nil,
      error = storage.fsim_error, completed = storage.fsim_completed or {}}
  end
})
