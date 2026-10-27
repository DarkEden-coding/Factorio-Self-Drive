local M = {}
function M.distance(a, b) return math.sqrt((a.x - b.x)^2 + (a.y - b.y)^2) end
function M.clamp(x, lo, hi) return math.max(lo, math.min(hi, x)) end
-- Rust stores field values and final observation features as f32. Factorio's
-- Lua lacks string.pack; implement IEEE round-to-nearest-even without bit casts.
function M.f32(x)
  if x == 0 or x == math.huge or x == -math.huge then return x end
  local sign = x < 0 and -1 or 1
  x = math.abs(x)
  local _, e = math.frexp(x)
  local step = 2 ^ math.max(e - 24, -149)
  local scaled = x / step
  local lo = math.floor(scaled)
  local r = scaled - lo
  if r > 0.5 or (r == 0.5 and lo % 2 == 1) then lo = lo + 1 end
  return sign * lo * step
end
function M.point_along(route, s)
  for i = 2, #route do
    local a, b = route[i - 1], route[i]
    local len = M.distance(a, b)
    if len >= s and len > 0 then
      return {x = a.x + s / len * (b.x - a.x), y = a.y + s / len * (b.y - a.y)}
    end
    s = s - len
  end
  return route[#route]
end
function M.length(route)
  local length = 0
  for i = 2, #route do length = length + M.distance(route[i - 1], route[i]) end
  return length
end
return M
