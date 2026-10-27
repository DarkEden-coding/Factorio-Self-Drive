local catalog = require("catalog")
local scenarios = {}
local function add(name, vehicle, options)
  local s = {name = name, surface = {tile = "grass-1", area = {{-64, -512}, {64, 512}}, water = {}},
    vehicle = {name = vehicle, position = {0, 0}, orientation = 0, speed = 0}, fuel = "coal",
    obstacles = {}, inputs = {}}
  for key, value in pairs(options) do s[key] = value end
  scenarios[#scenarios + 1] = s
end
for _, vehicle in ipairs(catalog.vehicles) do
  for _, tile in ipairs({"grass-1", "dirt-1", "sand-1"}) do
    for _, fuel in ipairs(catalog.fuels) do
      add(vehicle .. "-accelerate-" .. tile .. "-" .. fuel, vehicle, {
        surface = {tile = tile, area = {{-32, -4096}, {32, 4096}}, water = {}}, fuel = fuel,
        vehicle = {name = vehicle, position = {0, 3500}, orientation = 0, speed = 0},
        inputs = {{ticks = 6000, throttle = 1, steer = 0}}})
    end
  end
  add(vehicle .. "-coast", vehicle, {
    vehicle = {name = vehicle, position = {0, 300}, orientation = 0, speed = 0.4},
    inputs = {{ticks = 9000, throttle = 0, steer = 0}}})
  add(vehicle .. "-brake-reverse", vehicle, {
    vehicle = {name = vehicle, position = {0, 0}, orientation = 0, speed = 0.4},
    inputs = {{ticks = 1500, throttle = -1, steer = 0}, {ticks = 600, throttle = 1, steer = 0}}})
  for _, speed in ipairs({0, 0.005, 0.05, 0.25, -0.15}) do
    for _, steer in ipairs({-1, 1}) do
      add(vehicle .. "-steer-" .. speed .. "-" .. steer, vehicle, {
        vehicle = {name = vehicle, position = {0, 0}, orientation = 0.002, speed = speed},
        inputs = {{ticks = 240, throttle = 0, steer = steer}, {ticks = 120, throttle = 0, steer = 0}}})
    end
  end
  for _, orientation in ipairs({0, 0.125}) do
    add(vehicle .. "-water-" .. orientation, vehicle, {
      surface = {tile = "grass-1", area = {{-64, -512}, {64, 512}},
        water = {{name = "water", area = {{-64, -512}, {64, -8}}}}},
      vehicle = {name = vehicle, position = {0, 0}, orientation = orientation, speed = 0.3},
      inputs = {{ticks = 120, throttle = 1, steer = 0}}})
  end
end
for _, obstacle in ipairs({"tree-02", "big-rock", "cliff"}) do
  for _, speed in ipairs({0.05, 0.2, 0.43}) do
    for _, orientation in ipairs({0, 0.0625}) do
      local x = math.sin(orientation * 2 * math.pi) * 8
      local y = -math.cos(orientation * 2 * math.pi) * 8
      -- Cliffs use their placement grid, including its half-tile Y offset.
      if obstacle == "cliff" then x, y = 0, -7.5 end
      add("car-impact-" .. obstacle .. "-" .. speed .. "-" .. orientation, "car", {
        vehicle = {name = "car", position = {0, 0}, orientation = orientation, speed = speed},
        obstacles = {{name = obstacle, kind = obstacle == "cliff" and "cliff" or
          (obstacle == "tree-02" and "tree" or "rock"), position = {x, y},
          cliff_orientation = obstacle == "cliff" and "west-to-east" or nil}},
        inputs = {{ticks = 240, throttle = 0, steer = 0}}})
    end
  end
end
for _, speed in ipairs({0.1, 0.23}) do
  add("tank-trees-" .. speed, "tank", {
    vehicle = {name = "tank", position = {0, 0}, orientation = 0, speed = speed},
    obstacles = {{name = "tree-02", kind = "tree", position = {0, -8}},
      {name = "tree-02", kind = "tree", position = {0, -12}},
      {name = "tree-02", kind = "tree", position = {0, -16}}},
    inputs = {{ticks = 300, throttle = 1, steer = 0}}})
  add("tank-rock-" .. speed, "tank", {
    vehicle = {name = "tank", position = {0, 0}, orientation = 0, speed = speed},
    obstacles = {{name = "big-rock", kind = "rock", position = {0, -8}}},
    inputs = {{ticks = 240, throttle = 0, steer = 0}}})
end
-- Short probes distinguish snapping, low-speed cutoff and motion/turn ordering.
for _, vehicle in ipairs(catalog.vehicles) do
  for _, orientation in ipairs({0.002, 0.014, 0.016, 0.124, 0.248}) do
    for _, speed in ipairs({0, 0.008, -0.008, 0.2}) do
      add(vehicle .. "-calibration-snap-" .. orientation .. "-" .. speed, vehicle, {
        vehicle = {name = vehicle, position = {0, 0}, orientation = orientation, speed = speed},
        inputs = {{ticks = 4, throttle = 0, steer = 0}}})
    end
  end
  for _, orientation in ipairs({0.13, 0.375, 0.62, 0.875}) do
    add(vehicle .. "-calibration-turn-" .. orientation, vehicle, {
      vehicle = {name = vehicle, position = {0, 0}, orientation = orientation, speed = 0},
      inputs = {{ticks = 30, throttle = 1, steer = 1}, {ticks = 100, throttle = -1, steer = -1},
        {ticks = 30, throttle = 0, steer = 0}}})
  end
end
-- Turn against a nearby obstacle to expose rotational contact handling.
for _, vehicle in ipairs(catalog.vehicles) do
  for _, speed in ipairs({0, 0.05, 0.2}) do
    for _, side in ipairs({-1, 1}) do
      add(vehicle .. "-calibration-contact-turn-" .. speed .. "-" .. side, vehicle, {
        surface = {tile = "grass-1", area = {{-32, -32}, {32, 32}}, water = {}},
        vehicle = {name = vehicle, position = {0, 0}, orientation = 0, speed = speed},
        obstacles = {{name = "big-rock", kind = "rock", position = {side * 2, -2}}},
        inputs = {{ticks = 120, throttle = 0, steer = side}}})
    end
  end
end
return scenarios
