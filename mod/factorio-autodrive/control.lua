local driver=require("driver")
local vehicle_api=require("vehicle")
local overlay=require("overlay")
local trains=require("trains")
-- train_parked: last parked status per train id (trains.parked_changed).
local function init()
  storage.autodrive=storage.autodrive or {}
  storage.train_parked=storage.train_parked or {}
end
script.on_init(init)
-- In-flight drives from an older version lack fields the driver now needs; drop them.
script.on_configuration_changed(function()
  for _,s in pairs(storage.autodrive or {}) do
    if s.vehicle and s.vehicle.valid then vehicle_api.brake(s.vehicle) end
  end
  storage.autodrive={}
  storage.train_parked=storage.train_parked or {}
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
  local avoid_rails=settings.get_player_settings(player)["factorio-autodrive-avoid-rails"].value
  local ok,state=pcall(driver.start,vehicle,position,{avoid_rails=avoid_rails})
  if not ok then player.print("Autodrive: "..tostring(state)); return end
  state.profile=settings.global["factorio-autodrive-profile"].value
  storage.autodrive[player.index]=state
  player.print("Autodrive: finding a path. Movement keys hand control back.")
end
local function tool(event)
  local player=game.get_player(event.player_index)
  if player and player.clear_cursor() then player.cursor_stack.set_stack({name="factorio-autodrive-target"}) end
end
script.on_event("factorio-autodrive-tool",tool)
script.on_event(defines.events.on_lua_shortcut,function(event)
  if event.prototype_name=="factorio-autodrive-target" then tool(event) end
end)
local function select(event)
  if event.item=="factorio-autodrive-target" then
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
-- Movement keys pan the map in map or remote view; only take over in the normal world view.
local function override(event)
  local player=game.get_player(event.player_index)
  if not player or player.render_mode~=defines.render_mode.game or player.controller_type==defines.controllers.remote then return end
  pause(event)
end
for _,control in ipairs({"move-up","move-down","move-left","move-right"}) do
  script.on_event("factorio-autodrive-override-"..control,override)
end
script.on_event("factorio-autodrive-resume",function(event)
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
  if s and s.status=="driving" then driver.dump(s,"factorio-autodrive/parity-"..event.player_index..".json") end
end)
local function dirty_at(surface,position)
  for _,s in pairs(storage.autodrive) do
    if s.vehicle.valid and s.vehicle.surface==surface then driver.dirty(s,position) end
  end
end
local function dirty(event)
  local entity=event.entity or event.destination
  if not entity or not entity.valid or entity.type=="entity-ghost" or entity.type=="tile-ghost" then return end
  dirty_at(entity.surface,entity.position)
end
-- Built, removed, turned (a non-square footprint changes) or cloned.
script.on_event({defines.events.on_entity_died,defines.events.on_player_mined_entity,
  defines.events.on_robot_mined_entity,defines.events.script_raised_destroy,
  defines.events.on_built_entity,defines.events.on_robot_built_entity,
  defines.events.script_raised_built,defines.events.script_raised_revive,
  defines.events.on_player_rotated_entity,defines.events.on_player_flipped_entity,
  defines.events.on_entity_cloned},dirty)
-- Landfill turns water into ground; mining it back turns it into water.
local function tiles_dirty(event)
  local surface=game.get_surface(event.surface_index)
  if not surface or not event.tiles then return end
  local done={}
  for _,tile in ipairs(event.tiles) do
    local key=math.floor(tile.position.x/16)..":"..math.floor(tile.position.y/16)
    if not done[key] then done[key]=true; dirty_at(surface,tile.position) end
  end
end
script.on_event({defines.events.on_player_built_tile,defines.events.on_robot_built_tile,
  defines.events.on_player_mined_tile,defines.events.on_robot_mined_tile,
  defines.events.script_raised_set_tiles},tiles_dirty)
-- A train that parks becomes an obstacle; one that leaves frees its tiles.
-- The driver applies these in batches (driver.train_dirty).
script.on_event(defines.events.on_train_changed_state,function(event)
  local train=event.train
  if not trains.parked_changed(train,storage.train_parked) then return end
  for _,s in pairs(storage.autodrive) do
    if s.vehicle.valid and s.vehicle.surface==train.front_stock.surface then
      for _,carriage in ipairs(train.carriages) do driver.train_dirty(s,carriage.position) end
    end
  end
end)
-- Coupling or splitting replaces trains; forget the old ids.
script.on_event(defines.events.on_train_created,function(event)
  if event.old_train_id_1 then storage.train_parked[event.old_train_id_1]=nil end
  if event.old_train_id_2 then storage.train_parked[event.old_train_id_2]=nil end
end)
script.on_event(defines.events.on_script_path_request_finished,function(event)
  for _,s in pairs(storage.autodrive) do driver.on_path(s,event) end
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
      if game.tick%12==0 and settings.get_player_settings(player)["factorio-autodrive-overlay"].value then overlay.draw(s,player) end
    end
  end
end)
