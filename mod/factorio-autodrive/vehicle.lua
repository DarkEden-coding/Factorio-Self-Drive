local data = require("geometry-data")
local M = {}
function M.stats(vehicle)
  local p = assert(data.vehicles[vehicle.name],"Only base car and tank are supported")
  local burner = vehicle.burner
  local fuel = burner and burner.currently_burning and burner.currently_burning.name
  if type(fuel) == "string" then fuel = prototypes.item[fuel] end
  if not fuel and burner then
    local inventory = burner.inventory
    for i=1,#inventory do
      local stack = inventory[i]
      if stack.valid_for_read then fuel = prototypes.item[stack.name]; break end
    end
  end
  local a,t = fuel and fuel.fuel_acceleration_multiplier or 1,fuel and fuel.fuel_top_speed_multiplier or 1
  local power = p.consumption*p.effectivity*a*t^2
  local grass = prototypes.tile["grass-1"].vehicle_friction_modifier
  local friction = p.friction*(1+(grass-1)*p.terrain_friction_modifier)
  local r = math.max(0,math.min(1,1-friction))
  local top = math.sqrt((2*power/(60*p.weight))*r^2/(1-r^2))/60
  local tile = vehicle.surface.get_tile(vehicle.position)
  return {tank_driving=p.tank_driving,weight=p.weight,max_health=p.max_health,
    rotation_speed=p.rotation_speed,braking_power=p.braking_power,power=power,top_speed=top,
    half_width=math.max(math.abs(p.collision_box[1][1]),math.abs(p.collision_box[2][1])),
    half_length=math.max(math.abs(p.collision_box[1][2]),math.abs(p.collision_box[2][2])),
    fuel_acceleration=a,fuel_top_speed=t,tile_friction=tile.prototype.vehicle_friction_modifier,
    terrain_friction_modifier=p.terrain_friction_modifier}
end
function M.apply(vehicle,action)
  local throttle,steer = math.floor(action/3)-1,action%3-1
  local acceleration = defines.riding.acceleration.nothing
  if throttle == 1 then
    acceleration = vehicle.speed<0 and defines.riding.acceleration.braking or defines.riding.acceleration.accelerating
  elseif throttle == -1 then
    acceleration = vehicle.speed>0 and defines.riding.acceleration.braking or defines.riding.acceleration.reversing
  end
  vehicle.riding_state = {acceleration=acceleration,
    direction=steer==-1 and defines.riding.direction.left or (steer==1 and defines.riding.direction.right or defines.riding.direction.straight)}
end
function M.brake(vehicle)
  vehicle.riding_state = {acceleration=defines.riding.acceleration.braking,direction=defines.riding.direction.straight}
end
function M.release(vehicle)
  vehicle.riding_state = {acceleration=defines.riding.acceleration.nothing,direction=defines.riding.direction.straight}
end
return M
