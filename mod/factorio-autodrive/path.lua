-- Follows waypoints from LuaSurface.request_path the way the simulator's
-- stand-in does (crates/fsim-core/src/pathfinder.rs): the route is the vehicle,
-- its projection on the waypoints, then the later waypoints. A new path is
-- requested when the vehicle is more than REPLAN_DEVIATION tiles off them or on
-- their last segment far from the target. Requests are asynchronous here; until
-- an answer arrives the old waypoints (or a straight line) stand in.
local u = require("math-util")
local M = {}
M.REPLAN_DEVIATION = 16
local MIN_INTERVAL = 60   -- ticks between requests while the old path is still usable
local RETRY_INTERVAL = 300 -- ticks before asking again after a failed request
local function closest_on_segment(a, b, p)
  local dx, dy = b.x - a.x, b.y - a.y
  local len2 = dx * dx + dy * dy
  local t = len2 > 0 and u.clamp(((p.x - a.x) * dx + (p.y - a.y) * dy) / len2, 0, 1) or 0
  return {x = a.x + t * dx, y = a.y + t * dy}
end
-- Closest point to p on segments from..: (segment, point, distance). Never goes
-- back, so a path that doubles back keeps its order.
local function project(points, from, p)
  local best, bp, bd = from, p, math.huge
  for k = from, #points - 1 do
    local q = closest_on_segment(points[k], points[k + 1], p)
    local d = u.distance(q, p)
    if d < bd then best, bp, bd = k, q, d end
  end
  return best, bp, bd
end
function M.new(target)
  return {target = target, waypoints = {}, segment = 1, request = nil, last_request = nil,
    retry_at = nil, failed = false, requests = 0, arrivals = 0}
end
function M.request(p, vehicle)
  local ok, id = pcall(vehicle.surface.request_path, {
    bounding_box = vehicle.prototype.collision_box, collision_mask = vehicle.prototype.collision_mask,
    start = vehicle.position, goal = p.target, force = vehicle.force, radius = 4,
    entity_to_ignore = vehicle,
    pathfind_flags = {cache = false, prefer_straight_paths = true, no_break = true}})
  p.last_request = game.tick
  p.requests = p.requests + 1
  if ok and id then p.request = id else p.failed, p.retry_at = true, game.tick + RETRY_INTERVAL end
end
-- on_script_path_request_finished. Returns true when this was our request.
function M.on_path(p, event)
  if p.request ~= event.id then return false end
  p.request = nil
  if event.path and #event.path > 0 then
    local w = {}
    for _, step in ipairs(event.path) do w[#w + 1] = {x = step.position.x, y = step.position.y} end
    if u.distance(w[#w], p.target) > 1e-6 then w[#w + 1] = {x = p.target.x, y = p.target.y} end
    p.waypoints, p.segment, p.failed, p.retry_at = w, 1, false, nil
    p.arrivals = p.arrivals + 1
  else
    p.failed = true
    p.retry_at = game.tick + (event.try_again_later and 60 or RETRY_INTERVAL)
  end
  return true
end
local function wants_request(p, position)
  if p.request then return false end
  if p.retry_at then return game.tick >= p.retry_at end
  local w = p.waypoints
  if #w < 2 then return true end
  local segment, point, deviation = project(w, p.segment, position)
  local at_end = segment + 1 >= #w and u.distance(point, p.target) > 1 and
    u.distance(position, p.target) > M.REPLAN_DEVIATION
  if deviation > M.REPLAN_DEVIATION then return true end
  return at_end and game.tick - (p.last_request or -math.huge) >= MIN_INTERVAL
end
-- The route from the vehicle along the current waypoints, asking for a new
-- path when needed. Without usable waypoints: a straight line to the target.
function M.route(p, vehicle)
  local position = {x = vehicle.position.x, y = vehicle.position.y}
  if wants_request(p, position) then M.request(p, vehicle) end
  local w = p.waypoints
  if #w < 2 then return {position, {x = p.target.x, y = p.target.y}} end
  local segment, point = project(w, p.segment, position)
  p.segment = segment
  local route = {position, point}
  for i = segment + 1, #w do route[#route + 1] = w[i] end
  return route
end
return M
