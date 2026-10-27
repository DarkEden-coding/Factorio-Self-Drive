-- Runs the production modules with fixed snapshots. Only the Lua 5.2 bit32
-- library is shimmed when the host interpreter is 5.3+.
if not bit32 then
  bit32={band=assert(load('return function(a,b) return a & b end'))(),
    bor=assert(load('return function(a,b) return a | b end'))()}
end
package.path='mod/fsim-autodrive/?.lua;'..package.path
local input=dofile(arg[1])
local raster=require('raster')
local observation=require('observation')
local network=require('network')
local function line(values)
  local row={}
  for i,v in ipairs(values) do row[i]=string.format('%.17g',v) end
  print(table.concat(row,','))
end
for _,case in ipairs(input) do
  local grid={blocks={}}
  for _,b in ipairs(case.blocks) do
    grid.blocks[raster.key(b.x,b.y)]=raster.build(b.x,b.y,b.tiles,b.entities)
  end
  -- The Rust route (the Lua one comes from the game's pathfinder) tests the route features.
  local obs=observation.build(grid,case.route,case.state,case.stats)
  local action,logits=network.forward(obs)
  line(obs); line(logits); print(action)
end
