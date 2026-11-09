-- Runs the production modules with fixed snapshots. Only the Lua 5.2 bit32
-- library is shimmed when the host interpreter is 5.3+.
if not bit32 then
  bit32={band=assert(load('return function(a,b) return a & b end'))(),
    bor=assert(load('return function(a,b) return a | b end'))()}
end
package.path=(arg[2] or 'mod/factorio-autodrive')..'/?.lua;'..package.path
local input=dofile(arg[1])
local raster=require('raster')
local observation=require('observation')
local network=require('network')
local field=require('field')
local function line(values)
  local row={}
  for i,v in ipairs(values) do row[i]=string.format('%.17g',v) end
  print(table.concat(row,','))
end
for _,case in ipairs(input) do
  local grid={blocks={}}
  for _,b in ipairs(case.blocks) do
    grid.blocks[raster.key(b.x,b.y)]=raster.build(b.x,b.y,b.tiles,b.entities,b.territory,b.friction)
  end
  -- Build the actual field and route rather than injecting Rust's route.
  local state, stats = case.state, case.stats
  local options={}
  for k,v in pairs(network.model.field or {}) do options[k]=v end
  options.smooth=network.model.route_source=='field_smooth'
  options.terrain_friction_modifier=stats.terrain_friction_modifier
  local f = field.new({x=state.x,y=state.y},state.target,stats.half_width,stats.tank_driving,case.bounds,nil,options)
  while not field.slice(f,function(x,y) return raster.mask(grid,x,y) end,4000,
    function(x,y) return raster.friction(grid,x,y) end) do end
  local route = field.route(f,{x=state.x,y=state.y})
  local obs=observation.build(grid,route,state,stats)
  local action,logits=network.forward(obs)
  line(obs); line(logits); print(action); line(f.values)
end
