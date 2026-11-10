-- Bounded, resumable 8-connected Dijkstra. All cursors/queues are plain tables
-- so saving during preparation or a route slice is safe.
local u = require("math-util")
local smoothing = require("route-smoothing")
local band = bit32.band
local M = {}
local SQRT2 = math.sqrt(2)
local steps = {{1,0,1},{-1,0,1},{0,1,1},{0,-1,1},
  {1,1,SQRT2},{-1,1,SQRT2},{1,-1,SQRT2},{-1,-1,SQRT2}}
-- A bucket pop and its neighbor relaxations cost more than one cell in the other phases.
local POP_COST = 2
local function index(f, x, y)
  if x < f.x0 or y < f.y0 or x >= f.x1 or y >= f.y1 then return nil end
  return (y - f.y0) * f.w + x - f.x0 + 1
end
function M.tile(f, x, y)
  local i = index(f, x, y)
  return i and f.values[i] or math.huge
end
-- Unit-width circular buckets. Every edge costs at least 1, so nodes in the
-- current distance bucket cannot relax one another. The ring exceeds the largest
-- edge cost for the configured tree/zone costs. No heap sifting is needed.
local function push(f, node, distance)
  local slot = math.floor(distance) % f.bucket_count + 1
  local ids, distances = f.bucket_nodes[slot], f.bucket_distances[slot]
  if not ids then ids, distances = {}, {}; f.bucket_nodes[slot], f.bucket_distances[slot] = ids, distances end
  ids[#ids + 1], distances[#distances + 1] = node, distance
  f.pending = f.pending + 1
end
local function pop(f)
  local slot = f.current % f.bucket_count + 1
  while not f.bucket_nodes[slot] or #f.bucket_nodes[slot] == 0 do
    f.current = f.current + 1
    slot = f.current % f.bucket_count + 1
  end
  local ids, distances = f.bucket_nodes[slot], f.bucket_distances[slot]
  f.pending = f.pending - 1
  return table.remove(ids), table.remove(distances)
end
-- Slow-ground cost for a tile with friction `tenths` (distance_field.rs friction_extra):
-- the vehicle's slowdown against grass, times friction_cost, rounded to f32 like Rust.
function M.friction_extra(f, tenths)
  if f.friction_cost <= 0 then return 0 end
  local function g(m) return 1 + (m - 1) * f.tfm end
  return u.f32(f.friction_cost * math.max(0, math.sqrt(g(tenths / 10) / g(1.6)) - 1))
end
-- `margin` defaults to the simulator's 48 tiles.
function M.new(position, target, half_width, tank, bounds, margin, options)
  options = options or {}
  margin = margin or options.margin or 48
  local f = {x0 = math.floor(math.min(position.x, target.x)) - margin,
    y0 = math.floor(math.min(position.y, target.y)) - margin,
    x1 = math.floor(math.max(position.x, target.x)) + margin + 1,
    y1 = math.floor(math.max(position.y, target.y)) + margin + 1,
    dilation = math.ceil(half_width), tank = tank, target = target,
    smooth = options.smooth or false, tree_cost = options.tank_tree_cost or 4,
    zone_cost = options.nest_zone_cost or 3, zone_margin = options.nest_zone_margin or 2,
    friction_cost = options.friction_cost or 0, tfm = options.terrain_friction_modifier or 1,
    -- Off unless the driver asked to avoid rails (not part of the trained model's options).
    rail_cost = options.rail_cost or 0, rail_margin = options.rail_cost and math.ceil(half_width) + 1 or 0,
    clearance_cost = options.clearance_cost or 0,
    bucket_nodes = {}, bucket_distances = {}, pending = 0, current = 0, cursor = 1, phase = "base"}
  if bounds then
    f.x0,f.y0=math.max(f.x0,bounds.x0),math.max(f.y0,bounds.y0)
    f.x1,f.y1=math.min(f.x1,bounds.x1),math.min(f.y1,bounds.y1)
  end
  f.w, f.h = f.x1 - f.x0, f.y1 - f.y0
  -- Tiles this far from anything that blocks (beyond the blocked dilation) cost clearance_cost.
  local clearance_margin = options.clearance_margin or 1
  f.near_margin = (f.clearance_cost > 0 and clearance_margin > 0) and f.dilation + clearance_margin or 0
  -- The base masks cover the field plus a border wide enough for every dilation.
  f.bucket_count = math.ceil(SQRT2 * (1 + f.tree_cost + f.zone_cost + f.rail_cost + M.friction_extra(f, 255)
    + f.clearance_cost)) + 2
  f.r = math.max(f.dilation, f.zone_margin, f.rail_margin, f.near_margin)
  f.bw, f.bh = f.w + 2 * f.r, f.h + 2 * f.r
  f.hard, f.tree, f.zone, f.rail = {}, {}, {}, {}
  return f
end
-- Phase cells: base mask per bordered cell, then a row pass and a column pass
-- of the separable box dilation. A cell is hard if any cell within `dilation`
-- (Chebyshev) is a hard obstacle (or a tree, for the car); trees within
-- `dilation` cost the tank extra; nest zones within 2 cost extra; so do rails
-- within rail_margin, when rail_cost is set.
local function base_cell(f, mask, i)
  local x = f.x0 - f.r + (i - 1) % f.bw
  local y = f.y0 - f.r + math.floor((i - 1) / f.bw)
  local m = mask(x, y)
  f.hard[i] = band(m, 90) ~= 0 or (not f.tank and band(m, 4) ~= 0)
  f.tree[i] = band(m, 4) ~= 0
  f.zone[i] = band(m, 32) ~= 0
  f.rail[i] = band(m, 128) ~= 0
end
local function row_cell(f, i)
  -- i walks the bordered rows over the field's columns only.
  local row, col = math.floor((i - 1) / f.w), (i - 1) % f.w
  local b = row * f.bw + col + f.r + 1
  local d, hard, tree, zone, rail, near = f.dilation, false, false, false, false, false
  for k = -f.r, f.r do
    local j = b + k
    if k >= -d and k <= d then
      hard = hard or f.hard[j]
      tree = tree or f.tree[j]
    end
    if k >= -f.zone_margin and k <= f.zone_margin then zone = zone or f.zone[j] end
    if k >= -f.rail_margin and k <= f.rail_margin then rail = rail or f.rail[j] end
    if k >= -f.near_margin and k <= f.near_margin then near = near or f.hard[j] end
  end
  f.rh[i], f.rt[i], f.rz[i], f.rr[i], f.rn[i] = hard, tree, zone, rail, near
end
local function col_cell(f, i, friction)
  local x, y = (i - 1) % f.w, math.floor((i - 1) / f.w)
  local d, hard, tree, zone, rail, near = f.dilation, false, false, false, false, false
  for k = -f.r, f.r do
    local j = (y + f.r + k) * f.w + x + 1
    if k >= -d and k <= d then
      hard = hard or f.rh[j]
      tree = tree or f.rt[j]
    end
    if k >= -f.zone_margin and k <= f.zone_margin then zone = zone or f.rz[j] end
    if k >= -f.rail_margin and k <= f.rail_margin then rail = rail or f.rr[j] end
    if k >= -f.near_margin and k <= f.near_margin then near = near or f.rn[j] end
  end
  f.flags[i] = hard
  local slow = (friction and f.friction_cost > 0) and M.friction_extra(f, friction(f.x0 + x, f.y0 + y)) or 0
  f.extra[i] = (tree and f.tank and f.tree_cost or 0) + (zone and f.zone_cost or 0) + slow
    + (rail and f.rail_cost or 0) + (near and f.clearance_cost or 0)
end
local function start_search(f)
  local t = f.target
  local tx, ty = math.floor(t.x), math.floor(t.y)
  local source = index(f, tx, ty)
  f.distances, f.done = {}, {}
  if source then
    local d = u.distance({x = tx + 0.5, y = ty + 0.5}, t)
    f.distances[source] = d
    push(f, source, d)
  end
  f.phase = "search"
end
local function search_step(f)
  local i, du = pop(f)
  if f.done[i] or du ~= f.distances[i] then return end
  f.done[i] = true
  local w, h = f.w, f.h
  local col, row = (i - 1) % w, math.floor((i - 1) / w)
  local flags, done, distances, extra = f.flags, f.done, f.distances, f.extra
  local own_extra = extra[i]
  for k = 1, 8 do
    local step = steps[k]
    local dx, dy = step[1], step[2]
    local x, y = col + dx, row + dy
    if x >= 0 and x < w and y >= 0 and y < h then
      local j = i + dy * w + dx
      if not flags[j] and not done[j] and
        (k <= 4 or (not flags[i + dx] and not flags[i + dy * w])) then
        local distance = du + step[3] * (1 + 0.5 * (own_extra + extra[j]))
        if distance < (distances[j] or math.huge) then
          distances[j] = distance
          push(f, j, distance)
        end
      end
    end
  end
end
-- Budget is in work units: one per cell in the cell phases, POP_COST per bucket pop.
-- `friction(x, y)` gives friction tenths per tile; needed only with friction_cost > 0.
-- Returns true once the field is ready.
function M.slice(f, mask, budget, friction)
  while budget > 0 do
    local phase = f.phase
    if phase == "base" then
      local n = f.bw * f.bh
      local last = math.min(n, f.cursor + budget - 1)
      for i = f.cursor, last do base_cell(f, mask, i) end
      budget = budget - (last - f.cursor + 1)
      f.cursor = last + 1
      if f.cursor > n then f.phase, f.cursor, f.rh, f.rt, f.rz, f.rr, f.rn = "rows", 1, {}, {}, {}, {}, {} end
    elseif phase == "rows" then
      local n = f.w * f.bh
      local last = math.min(n, f.cursor + budget - 1)
      for i = f.cursor, last do row_cell(f, i) end
      budget = budget - (last - f.cursor + 1)
      f.cursor = last + 1
      if f.cursor > n then
        f.phase, f.cursor, f.flags, f.extra = "cols", 1, {}, {}
        f.hard, f.tree, f.zone, f.rail = nil, nil, nil, nil
      end
    elseif phase == "cols" then
      local n = f.w * f.h
      local last = math.min(n, f.cursor + budget - 1)
      for i = f.cursor, last do col_cell(f, i, friction) end
      budget = budget - (last - f.cursor + 1)
      f.cursor = last + 1
      if f.cursor > n then
        f.rh, f.rt, f.rz, f.rr, f.rn = nil, nil, nil, nil, nil
        start_search(f)
      end
    elseif phase == "search" then
      if f.pending == 0 then
        f.phase, f.cursor, f.values = "values", 1, {}
      else
        search_step(f)
        budget = budget - POP_COST
      end
    elseif phase == "values" then
      local n = f.w * f.h
      local last = math.min(n, f.cursor + budget - 1)
      local values, distances = f.values, f.distances
      for i = f.cursor, last do values[i] = u.f32(distances[i] or math.huge) end
      budget = budget - (last - f.cursor + 1)
      f.cursor = last + 1
      if f.cursor > n then
        f.phase = "ready"
        -- Corridor verification also needs costs when grid-angle guidance is selected.
        f.route_flags,f.route_extra=f.flags,f.extra
        f.bucket_nodes, f.bucket_distances, f.distances, f.done, f.flags, f.extra = nil, nil, nil, nil, nil, nil
        return true
      end
    else
      return true
    end
  end
  return f.phase == "ready"
end
function M.sample(f, x, y)
  local ix, iy = math.floor(x - 0.5), math.floor(y - 0.5)
  local tx, ty = x - 0.5 - ix, y - 0.5 - iy
  local sum, weight = 0, 0
  for _, c in ipairs({{0,0,(1-tx)*(1-ty)},{1,0,tx*(1-ty)},{0,1,(1-tx)*ty},{1,1,tx*ty}}) do
    local v = M.tile(f, ix + c[1], iy + c[2])
    if v < math.huge and c[3] > 0 then sum, weight = sum + v * c[3], weight + c[3] end
  end
  if weight > 1e-9 then return sum / weight end
  local best = math.huge
  for ny = math.floor(y) - 3, math.floor(y) + 3 do
    for nx = math.floor(x) - 3, math.floor(x) + 3 do
      best = math.min(best, M.tile(f, nx, ny) + u.distance({x=nx+0.5,y=ny+0.5},{x=x,y=y}))
    end
  end
  return best
end
local function grid_route(f, position)
  local cx, cy = math.floor(position.x), math.floor(position.y)
  local route = {{x=position.x,y=position.y}}
  if M.tile(f, cx, cy) == math.huge then
    local best, bx, by = math.huge, cx, cy
    for ny = cy - 3, cy + 3 do
      for nx = cx - 3, cx + 3 do
        local cost = M.tile(f,nx,ny) + u.distance({x=nx+0.5,y=ny+0.5},position)
        if cost < best then best, bx, by = cost, nx, ny end
      end
    end
    if best == math.huge then route[#route+1] = f.target; return route end
    cx, cy = bx, by
    route[#route+1] = {x=cx+0.5,y=cy+0.5}
  end
  for _ = 1, f.w * f.h do
    local best, bx, by = M.tile(f,cx,cy), cx, cy
    for k, step in ipairs(steps) do
      local dx, dy = step[1], step[2]
      if k <= 4 or (M.tile(f,cx+dx,cy) < math.huge and M.tile(f,cx,cy+dy) < math.huge) then
        local v = M.tile(f,cx+dx,cy+dy)
        if v < best then best, bx, by = v, cx+dx, cy+dy end
      end
    end
    if bx == cx and by == cy then break end
    cx, cy = bx, by
    route[#route+1] = {x=cx+0.5,y=cy+0.5}
  end
  route[#route+1] = f.target
  return route
end
-- nil means outside this section; infinity means a known blocked cell.
function M.cost(f,x,y)
  local i=index(f,x,y)
  if not i then return nil end
  if not f.route_flags or f.route_flags[i] then return math.huge end
  return 1+f.route_extra[i]
end
function M.route(f,position)
  local route=grid_route(f,position)
  if not f.smooth then return route end
  return smoothing.route(route,function(x,y)
    local i=index(f,x,y)
    if not i or f.route_flags[i] then return math.huge end
    return 1+f.route_extra[i]
  end)
end
return M
