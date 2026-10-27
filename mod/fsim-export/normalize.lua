local M = {}
local array_fields = {resistances = true, collision_mask = true, water = true, obstacles = true,
  obstacle_health = true, resources = true, entities = true, tiles = true, inputs = true,
  filters = true, completed = true}
function M.json(value)
  -- Factorio serializes an empty Lua table as {}, even for schema array fields.
  return (helpers.table_to_json(value):gsub('"([%w_]+)"%s*:%s*{}', function(key)
    return '"' .. key .. '":' .. (array_fields[key] and "[]" or "{}")
  end))
end
function M.position(p) return {p.x or p[1], p.y or p[2]} end
function M.box(b)
  return {M.position(b.left_top or b[1]), M.position(b.right_bottom or b[2])}
end
function M.color(c)
  local r, g, b = c.r or c[1] or 0, c.g or c[2] or 0, c.b or c[3] or 0
  local scale = math.max(r, g, b, c.a or c[4] or 0) <= 1 and 255 or 1
  return {math.floor(r * scale + 0.5), math.floor(g * scale + 0.5), math.floor(b * scale + 0.5)}
end
function M.kind(t)
  return ({tree = "tree", ["simple-entity"] = "rock", cliff = "cliff", ["unit-spawner"] = "nest"})[t]
end
function M.source()
  return {game_version = helpers.game_version, active_mods = script.active_mods}
end
function M.write(path, value, player)
  M.write_file(path, M.json(value), false, player)
end
function M.write_file(path, value, append, player)
  -- false is reserved for isolated benchmark jobs: there is no server in that mode.
  if player == false then helpers.write_file(path, value, append)
  else helpers.write_file(path, value, append, player or 0) end
end
return M
