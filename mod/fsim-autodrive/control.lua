local driver=require("driver")
local test_runner=require("test-runner")
local vehicle_api=require("vehicle")
local grid=require("grid")
local overlay=require("overlay")
local function init() storage.autodrive=storage.autodrive or {} end
script.on_init(init)
-- In-flight drives from an older version lack fields the driver now needs; drop them.
script.on_configuration_changed(function()
  storage.autodrive={}
end)
-- physical_vehicle, not vehicle: in map/remote view player.vehicle is nil even
-- though the player still sits in the car.
local function owned_vehicle(player)
  local vehicle=player.physical_vehicle
  if not vehicle or not vehicle.valid or (vehicle.name~="car" and vehicle.name~="tank") then return nil end
  local seated=vehicle.get_driver()
  if seated==player or seated==player.character then return vehicle end
end
local function target(player,position,surface)
  local vehicle=owned_vehicle(player)
  if not vehicle then player.print("Autodrive: sit in the driver's seat of a car or tank."); return end
  if surface and surface~=vehicle.surface then player.print("Autodrive: target must be on your surface."); return end
  if position.x~=position.x or position.y~=position.y or math.abs(position.x)>1e7 or math.abs(position.y)>1e7 then
    player.print("Autodrive: invalid coordinates."); return
  end
  local ok,state=pcall(driver.start,vehicle,position)
  if not ok then player.print("Autodrive: "..tostring(state)); return end
  state.profile=settings.global["fsim-autodrive-profile"].value
  storage.autodrive[player.index]=state
  player.print("Autodrive: finding a path. Movement keys hand control back.")
end
local function tool(event)
  local player=game.get_player(event.player_index)
  if player and player.clear_cursor() then player.cursor_stack.set_stack({name="fsim-autodrive-target"}) end
end
script.on_event("fsim-autodrive-tool",tool)
script.on_event(defines.events.on_lua_shortcut,function(event)
  if event.prototype_name=="fsim-autodrive-target" then tool(event) end
end)
local function select(event)
  if event.item=="fsim-autodrive-target" then
    local player=game.get_player(event.player_index)
    target(player,event.cursor_position,event.surface)
  end
end
script.on_event(defines.events.on_player_selected_area,select)
script.on_event(defines.events.on_player_alt_selected_area,select)
local function pause(event)
  local s=storage.autodrive[event.player_index]
  if s and s.status~="paused" then
    s.status="paused"
    if s.vehicle.valid and owned_vehicle(game.get_player(event.player_index))==s.vehicle then vehicle_api.release(s.vehicle) end
    local player=game.get_player(event.player_index)
    player.print("Autodrive: manual control. Ctrl+Alt+D resumes.")
  end
end
for _,control in ipairs({"move-up","move-down","move-left","move-right"}) do
  script.on_event("fsim-autodrive-override-"..control,pause)
end
script.on_event("fsim-autodrive-resume",function(event)
  local player=game.get_player(event.player_index)
  local s=storage.autodrive[event.player_index]
  if s then target(player,s.target) else tool(event) end
end)
commands.add_command("autodrive","<x> <y> or stop",function(event)
  if not event.player_index then return end
  local player=game.get_player(event.player_index)
  if event.parameter=="stop" then pause(event); return end
  local x,y=(event.parameter or ""):match("^%s*([%+%-%.%deE]+)%s+([%+%-%.%deE]+)%s*$")
  x,y=tonumber(x),tonumber(y)
  if x and y then target(player,{x=x,y=y}) else player.print("Usage: /autodrive <x> <y> or /autodrive stop") end
end)
commands.add_command("autodrive-dump","Write a parity snapshot of the current route and state",function(event)
  local s=event.player_index and storage.autodrive[event.player_index]
  if s and s.status=="driving" then driver.dump(s,"fsim-autodrive/parity-"..event.player_index..".json") end
end)
local function dirty(event)
  local entity=event.entity
  if not entity or not entity.valid then return end
  if storage.autodrive_test and storage.autodrive_test.state then
    local s=storage.autodrive_test.state
    if s.vehicle.valid and s.vehicle.surface==entity.surface then grid.dirty(s.grid,entity.position) end
  end
  for _,s in pairs(storage.autodrive) do
    if s.vehicle.valid and s.vehicle.surface==entity.surface then grid.dirty(s.grid,entity.position) end
  end
end
script.on_event({defines.events.on_entity_died,defines.events.on_player_mined_entity,
  defines.events.on_robot_mined_entity,defines.events.script_raised_destroy},dirty)
script.on_event(defines.events.on_script_path_request_finished,function(event)
  for _,s in pairs(storage.autodrive) do driver.on_path(s,event) end
  if storage.autodrive_test and storage.autodrive_test.state then driver.on_path(storage.autodrive_test.state,event) end
end)
script.on_event(defines.events.on_tick,function()
  for index,s in pairs(storage.autodrive) do
    local player=game.get_player(index)
    if not player or not player.connected or owned_vehicle(player)~=s.vehicle then
      if s.vehicle.valid and not s.vehicle.get_driver() and s.status~="paused" then vehicle_api.release(s.vehicle) end
      storage.autodrive[index]=nil
    else
      local result=driver.tick(s)
      if result then player.print("Autodrive: "..result..".") end
      if game.tick%12==0 and settings.get_player_settings(player)["fsim-autodrive-overlay"].value then overlay.draw(s,player) end
    end
  end
  if storage.autodrive_test then test_runner.tick() end
end)
-- Explicit opt-in verification only; never creates test surfaces in normal play.
remote.add_interface("fsim-autodrive-test",{start=function() test_runner.start() end})
