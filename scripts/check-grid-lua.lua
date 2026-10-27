-- The narrower obstacle query must retain masks at block edges and distant nest zones.
if not bit32 then
  bit32={band=assert(load('return function(a,b) return a & b end'))(),
    bor=assert(load('return function(a,b) return a | b end'))()}
end
package.path='mod/fsim-autodrive/?.lua;'..package.path
local grid,raster=require('grid'),require('raster')
local entities,reference,queries={},{},{}
local function add(kind,x,y,box,cliff)
  local names={tree='tree',rock='simple-entity',cliff='cliff',nest='unit-spawner'}
  entities[#entities+1]={type=names[kind],position={x=x,y=y},orientation=0,health=50,
    cliff_orientation=cliff,prototype={count_as_rock_for_filtered_deconstruction=true,
      collision_box={left_top={x=box[1][1],y=box[1][2]},right_bottom={x=box[2][1],y=box[2][2]}}}}
  reference[#reference+1]={kind=kind,x=x,y=y,box=box,health=50,orientation=0,cliff_orientation=cliff}
end
for y=-24,40,2 do for x=-24,40,2 do add('tree',x+0.8,y+0.8,{{-0.4,-0.4},{0.4,0.4}}) end end
add('rock',-0.2,7,{{-1,-1},{1,1}})
add('cliff',17,10,{{-2,-2},{2,2}},'west-to-north')
add('nest',-12,8,{{-2.4,-2},{2.4,2}})
local surface={get_tile=function() return {name='grass-1',collides_with=function() return false end} end,
  find_entities_filtered=function(q)
    queries[#queries+1]=q
    local result,types={},{}
    for _,t in ipairs(q.type) do types[t]=true end
    for _,e in ipairs(entities) do
      local p,a=e.position,q.area
      if types[e.type] and p.x>=a[1][1] and p.x<a[2][1] and p.y>=a[1][2] and p.y<a[2][2] then
        result[#result+1]=e
      end
    end
    return result
  end}
local g=grid.new()
grid.ensure(g,0,0,16,16,true)
assert(grid.slice(g,surface,1))
local expected=raster.build(0,0,{},reference)
for y=0,15 do for x=0,15 do assert(grid.mask(g,x,y)==expected.mask[y*16+x+1]) end end
assert(raster.in_zone(g,0,8)==1,'nest outside the obstacle query must retain its zone')
assert(#queries==2 and queries[1].area[1][1]==-8 and queries[2].area[1][1]==-26)
print('Grid cache masks match the broad-query reference, including block edges and nest zones')
