-- Bounded, resumable 8-connected Dijkstra. All cursors/queues are plain tables
-- so saving during preparation or a route slice is safe.
local u = require("math-util")
local band = bit32.band
local M = {}
local SQRT2 = math.sqrt(2)
local steps = {{1,0,1},{-1,0,1},{0,1,1},{0,-1,1},
  {1,1,SQRT2},{-1,1,SQRT2},{1,-1,SQRT2},{-1,-1,SQRT2}}
-- A heap pop does several times the work of one cell in the other phases.
local POP_COST = 4
local ZONE_REACH = 2
local function index(f, x, y)
  if x < f.x0 or y < f.y0 or x >= f.x1 or y >= f.y1 then return nil end
  return (y - f.y0) * f.w + x - f.x0 + 1
end
function M.tile(f, x, y)
  local i = index(f, x, y)
  return i and f.values[i] or math.huge
end
-- Binary heap in two parallel arrays (node index, distance). Same sift order as
-- a heap of {i, d} records, so ties pop in the same order.
local function push(f, node, distance)
  local hi, hd = f.hi, f.hd
  local i = f.hn + 1
  f.hn = i
  while i > 1 do
    local p = math.floor(i / 2)
    if hd[p] <= distance then break end
    hi[i], hd[i], i = hi[p], hd[p], p
  end
  hi[i], hd[i] = node, distance
end
local function pop(f)
  local hi, hd, n = f.hi, f.hd, f.hn
  local first_i, first_d = hi[1], hd[1]
  local last_i, last_d = hi[n], hd[n]
  hi[n], hd[n] = nil, nil
  n = n - 1
  f.hn = n
  if n > 0 then
    local i = 1
    while i * 2 <= n do
      local child = i * 2
      if child < n and hd[child + 1] < hd[child] then child = child + 1 end
      if last_d <= hd[child] then break end
      hi[i], hd[i], i = hi[child], hd[child], child
    end
    hi[i], hd[i] = last_i, last_d
  end
  return first_i, first_d
end
-- `margin` defaults to the simulator's 48 tiles.
function M.new(position, target, half_width, tank, bounds, margin)
  margin = margin or 48
  local f = {x0 = math.floor(math.min(position.x, target.x)) - margin,
    y0 = math.floor(math.min(position.y, target.y)) - margin,
    x1 = math.floor(math.max(position.x, target.x)) + margin + 1,
    y1 = math.floor(math.max(position.y, target.y)) + margin + 1,
    dilation = math.ceil(half_width), tank = tank, target = target,
    hi = {}, hd = {}, hn = 0, cursor = 1, phase = "base"}
  if bounds then
    f.x0,f.y0=math.max(f.x0,bounds.x0),math.max(f.y0,bounds.y0)
    f.x1,f.y1=math.min(f.x1,bounds.x1),math.min(f.y1,bounds.y1)
  end
  f.w, f.h = f.x1 - f.x0, f.y1 - f.y0
  -- The base masks cover the field plus a border wide enough for both dilations.
  f.r = math.max(f.dilation, ZONE_REACH)
  f.bw, f.bh = f.w + 2 * f.r, f.h + 2 * f.r
  f.hard, f.tree, f.zone = {}, {}, {}
  return f
end
-- Phase cells: base mask per bordered cell, then a row pass and a column pass
-- of the separable box dilation. A cell is hard if any cell within `dilation`
-- (Chebyshev) is a hard obstacle (or a tree, for the car); trees within
-- `dilation` cost the tank extra; nest zones within 2 cost extra.
local function base_cell(f, mask, i)
  local x = f.x0 - f.r + (i - 1) % f.bw
  local y = f.y0 - f.r + math.floor((i - 1) / f.bw)
  local m = mask(x, y)
  f.hard[i] = band(m, 90) ~= 0 or (not f.tank and band(m, 4) ~= 0)
  f.tree[i] = band(m, 4) ~= 0
  f.zone[i] = band(m, 32) ~= 0
end
local function row_cell(f, i)
  -- i walks the bordered rows over the field's columns only.
  local row, col = math.floor((i - 1) / f.w), (i - 1) % f.w
  local b = row * f.bw + col + f.r + 1
  local d, hard, tree, zone = f.dilation, false, false, false
  for k = -f.r, f.r do
    local j = b + k
    if k >= -d and k <= d then
      hard = hard or f.hard[j]
      tree = tree or f.tree[j]
    end
    if k >= -ZONE_REACH and k <= ZONE_REACH then zone = zone or f.zone[j] end
  end
  f.rh[i], f.rt[i], f.rz[i] = hard, tree, zone
end
local function col_cell(f, i)
  local x, y = (i - 1) % f.w, math.floor((i - 1) / f.w)
  local d, hard, tree, zone = f.dilation, false, false, false
  for k = -f.r, f.r do
    local j = (y + f.r + k) * f.w + x + 1
    if k >= -d and k <= d then
      hard = hard or f.rh[j]
      tree = tree or f.rt[j]
    end
    if k >= -ZONE_REACH and k <= ZONE_REACH then zone = zone or f.rz[j] end
  end
  f.flags[i] = hard
  f.extra[i] = (tree and f.tank and 4 or 0) + (zone and 3 or 0)
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
  local w = f.w
  local x, y = f.x0 + (i - 1) % w, f.y0 + math.floor((i - 1) / w)
  local flags, done, distances, extra = f.flags, f.done, f.distances, f.extra
  for k = 1, 8 do
    local step = steps[k]
    local dx, dy = step[1], step[2]
    local j = index(f, x + dx, y + dy)
    if j and not flags[j] and not done[j] then
      local ok = k <= 4
      if not ok then
        local ox, oy = index(f, x + dx, y), index(f, x, y + dy)
        ok = ox and oy and not flags[ox] and not flags[oy]
      end
      if ok then
        local distance = du + step[3] * (1 + 0.5 * (extra[i] + extra[j]))
        if distance < (distances[j] or math.huge) then
          distances[j] = distance
          push(f, j, distance)
        end
      end
    end
  end
end
-- Budget is in work units: one per cell in the cell phases, POP_COST per heap pop.
-- Returns true once the field is ready.
function M.slice(f, mask, budget)
  while budget > 0 do
    local phase = f.phase
    if phase == "base" then
      local n = f.bw * f.bh
      local last = math.min(n, f.cursor + budget - 1)
      for i = f.cursor, last do base_cell(f, mask, i) end
      budget = budget - (last - f.cursor + 1)
      f.cursor = last + 1
      if f.cursor > n then f.phase, f.cursor, f.rh, f.rt, f.rz = "rows", 1, {}, {}, {} end
    elseif phase == "rows" then
      local n = f.w * f.bh
      local last = math.min(n, f.cursor + budget - 1)
      for i = f.cursor, last do row_cell(f, i) end
      budget = budget - (last - f.cursor + 1)
      f.cursor = last + 1
      if f.cursor > n then
        f.phase, f.cursor, f.flags, f.extra = "cols", 1, {}, {}
        f.hard, f.tree, f.zone = nil, nil, nil
      end
    elseif phase == "cols" then
      local n = f.w * f.h
      local last = math.min(n, f.cursor + budget - 1)
      for i = f.cursor, last do col_cell(f, i) end
      budget = budget - (last - f.cursor + 1)
      f.cursor = last + 1
      if f.cursor > n then
        f.rh, f.rt, f.rz = nil, nil, nil
        start_search(f)
      end
    elseif phase == "search" then
      if f.hn == 0 then
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
        f.hi, f.hd, f.distances, f.done, f.flags, f.extra = nil, nil, nil, nil, nil, nil
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
function M.route(f, position)
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
return M
