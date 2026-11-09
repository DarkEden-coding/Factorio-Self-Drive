-- The narrower obstacle query must retain masks at block edges and distant nest zones.
if not bit32 then
  bit32={band=assert(load('return function(a,b) return a & b end'))(),
    bor=assert(load('return function(a,b) return a | b end'))()}
end
package.path='mod/factorio-autodrive/?.lua;'..package.path
-- Engine stubs: the car's default collision mask and tile/obstacle prototypes.
prototypes={entity={car={collision_mask={layers={player=true,train=true,car=true,is_object=true}}}}}
local OBJECT={layers={object=true,player=true,is_object=true}}
local function tile(name,friction,layers)
  return {name=name,prototype={name=name,vehicle_friction_modifier=friction,collision_mask={layers=layers}}}
end
local GRASS=tile('grass-1',1.6,{ground_tile=true})
local grid,raster=require('grid'),require('raster')
local entities,reference,queries={},{},{}
local function add(kind,x,y,box,cliff)
  local names={tree='tree',rock='simple-entity',cliff='cliff',nest='unit-spawner'}
  entities[#entities+1]={type=names[kind],position={x=x,y=y},orientation=0,health=50,
    cliff_orientation=cliff,name=kind,prototype={name=kind,collision_mask=OBJECT,
      collision_box={left_top={x=box[1][1],y=box[1][2]},right_bottom={x=box[2][1],y=box[2][2]}}}}
  reference[#reference+1]={kind=kind,x=x,y=y,box=box,health=50,orientation=0,cliff_orientation=cliff}
end
for y=-24,40,2 do for x=-24,40,2 do add('tree',x+0.8,y+0.8,{{-0.4,-0.4},{0.4,0.4}}) end end
add('rock',-0.2,7,{{-1,-1},{1,1}})
add('cliff',17,10,{{-2,-2},{2,2}},'west-to-north')
add('nest',-12,8,{{-2.4,-2},{2.4,2}})
local surface={get_tile=function() return GRASS end,
  get_territory_for_chunk=function() return nil end,
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
-- Space Age: lava blocks, oil ocean is open but slow, Gleba plants are trees, territory is zone.
local LAVA=tile('lava',1,{lava_tile=true,player=true,water_tile=true,item=true})
local OIL=tile('oil-ocean-deep',10,{water_tile=true,resource=true,rail_support=true})
local planet={get_tile=function(x) return x<4 and LAVA or OIL end,
  get_territory_for_chunk=function(c) return c[1]==2 and {} or nil end,
  find_entities_filtered=function(q)
    local found={}
    for _,t in ipairs(q.type) do
      if t=='plant' and q.area[1][1]<=36 and q.area[2][1]>36 then
        found[#found+1]={type='plant',name='yumako-tree',position={x=36.5,y=8.5},orientation=0,health=50,
          prototype={name='yumako-tree',collision_mask=OBJECT,
            collision_box={left_top={x=-0.8,y=-0.8},right_bottom={x=0.8,y=0.8}}}}
      end
    end
    return found
  end}
local pg=grid.new()
grid.ensure(pg,0,0,16,16,true); grid.ensure(pg,64,0,80,16,true); grid.ensure(pg,32,0,48,16,true)
while not grid.slice(pg,planet,1) do end
assert(raster.hard(grid.mask(pg,2,2)) and not raster.hard(grid.mask(pg,6,2)),'lava blocks, oil ocean does not')
assert(raster.friction(pg,6,2)==100 and raster.friction(pg,2,2)==10,'friction tenths per tile')
assert(bit32.band(grid.mask(pg,36,8),4)~=0,'Gleba plants are trees')
assert(raster.in_zone(pg,70,5)==1 and bit32.band(grid.mask(pg,70,5),32)~=0,'territory chunks are zone')
assert(raster.in_zone(pg,36,5)==0,'outside territory is not zone')
print('Grid cache masks match the broad-query reference, including block edges and nest zones')
print('Planet tiles, plants, friction and demolisher territory pass')
