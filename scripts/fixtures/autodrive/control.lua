-- Appended only to a disposable mod copy. These handlers replace player driving
-- while the verification companion runs in a playerless save.
local test_runner=require("test-runner")
script.on_event(defines.events.on_tick,function()
  if storage.autodrive_test then test_runner.tick() end
end)
script.on_event(defines.events.on_script_path_request_finished,function(event)
  local job=storage.autodrive_test
  if job and job.state then driver.on_path(job.state,event) end
end)
script.on_event({defines.events.on_entity_died,defines.events.on_player_mined_entity,
  defines.events.on_robot_mined_entity,defines.events.script_raised_destroy},function(event)
  local entity=event.entity
  local job=storage.autodrive_test
  local s=job and job.state
  if entity and entity.valid and s and s.vehicle.valid and s.vehicle.surface==entity.surface then
    driver.dirty(s,entity.position)
  end
end)
remote.add_interface("fsim-autodrive-test",{start=function(options) test_runner.start(options) end})
