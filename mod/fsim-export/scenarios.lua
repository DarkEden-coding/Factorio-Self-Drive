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
-- Space Age planets: surface properties match the planet; tiles and obstacles are its own.
if script.active_mods["space-age"] then
  local planets = {
    vulcanus = {properties = {gravity = 40, pressure = 4000}, ground = "volcanic-soil-dark"},
    gleba = {properties = {gravity = 20, pressure = 2000}, ground = "lowland-olive-blubber"},
    fulgora = {properties = {gravity = 8, pressure = 800}, ground = "fulgoran-rock"}
  }
  local function planet_surface(planet, tile, area, patches)
    return {tile = tile, area = area or {{-32, -4096}, {32, 4096}}, water = patches or {},
      properties = planets[planet].properties}
  end
  local accelerate = {
    {"vulcanus", "volcanic-soil-dark"}, {"vulcanus", "volcanic-folds"},
    {"gleba", "lowland-olive-blubber"}, {"gleba", "wetland-blue-slime"},
    {"fulgora", "fulgoran-rock"}, {"fulgora", "fulgoran-paving"},
    {"fulgora", "oil-ocean-shallow"}, {"fulgora", "oil-ocean-deep"}}
  for _, vehicle in ipairs(catalog.vehicles) do
    for _, case in ipairs(accelerate) do
      add(vehicle .. "-accelerate-" .. case[2] .. "-coal", vehicle, {
        surface = planet_surface(case[1], case[2]),
        vehicle = {name = vehicle, position = {0, 3500}, orientation = 0, speed = 0},
        inputs = {{ticks = 3000, throttle = 1, steer = 0}}})
    end
    -- Mixed ground: drive from Fulgora land into the oil ocean and back out of it.
    add(vehicle .. "-cross-oil-ocean", vehicle, {
      surface = planet_surface("fulgora", "fulgoran-rock", {{-64, -512}, {64, 512}},
        {{name = "oil-ocean-deep", area = {{-64, -200}, {64, -100}}}}),
      vehicle = {name = vehicle, position = {0, 0}, orientation = 0, speed = 0},
      inputs = {{ticks = 1500, throttle = 1, steer = 0}}})
    for _, wall in ipairs({{"vulcanus", "lava"}, {"vulcanus", "lava-hot"}, {"gleba", "gleba-deep-lake"}}) do
      for _, orientation in ipairs({0, 0.125}) do
        add(vehicle .. "-" .. wall[2] .. "-" .. orientation, vehicle, {
          surface = planet_surface(wall[1], planets[wall[1]].ground, {{-64, -512}, {64, 512}},
            {{name = wall[2], area = {{-64, -512}, {64, -8}}}}),
          vehicle = {name = vehicle, position = {0, 0}, orientation = orientation, speed = 0.3},
          inputs = {{ticks = 120, throttle = 1, steer = 0}}})
      end
    end
  end
  local impacts = {
    {"vulcanus", "ashland-lichen-tree", "tree"}, {"vulcanus", "huge-volcanic-rock", "rock"},
    {"vulcanus", "vulcanus-chimney", "rock"}, {"vulcanus", "cliff-vulcanus", "cliff"},
    {"vulcanus", "crater-cliff", "cliff"},
    {"gleba", "water-cane", "tree"}, {"gleba", "funneltrunk", "tree"}, {"gleba", "yumako-tree", "tree"},
    {"gleba", "hairyclubnub", "tree"}, {"gleba", "iron-stromatolite", "rock"}, {"gleba", "cliff-gleba", "cliff"},
    {"fulgora", "fulgoran-ruin-big", "rock"}, {"fulgora", "fulgurite", "rock"},
    {"fulgora", "fulgoran-ruin-attractor", "rock"}, {"fulgora", "cliff-fulgora", "cliff"}}
  for _, case in ipairs(impacts) do
    for _, vehicle in ipairs(catalog.vehicles) do
      for _, speed in ipairs({0.1, 0.3}) do
        local cliff = case[3] == "cliff"
        add(vehicle .. "-impact-" .. case[2] .. "-" .. speed, vehicle, {
          surface = planet_surface(case[1], planets[case[1]].ground, {{-64, -512}, {64, 512}}),
          vehicle = {name = vehicle, position = {0, 0}, orientation = 0, speed = speed},
          obstacles = {{name = case[2], kind = case[3], position = cliff and {0, -7.5} or {0, -8},
            cliff_orientation = cliff and "west-to-east" or nil}},
          inputs = {{ticks = 240, throttle = 1, steer = 0}}})
      end
    end
  end
end
return scenarios
