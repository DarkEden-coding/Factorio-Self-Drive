-- Exercise the production rolling corridor without Factorio or a policy stub.
if not bit32 then bit32={band=assert(load('return function(a,b) return a & b end'))()} end
package.path='mod/factorio-autodrive/?.lua;'..package.path
local field,corridor,u=require('field'),require('corridor'),require('math-util')
local function plan(from,target,mask)
  local f=field.new(from,target,0,false,nil,16,{smooth=true})
  local slices=0
  repeat slices=slices+1 until field.slice(f,mask or function() return 0 end,100)
  assert(slices>1,'field work must remain resumable')
  return {field=f,from=from,target=target}
end
local a=plan({x=0.5,y=0.5},{x=40.5,y=10.5})
local b=plan({x=20.5,y=5.5},{x=100.5,y=0.5})
local c=plan({x=80.5,y=3.5},{x=180.5,y=30.5})
local first=corridor.refine({a},a.from)
local next=corridor.refine({a,b},a.from,first)
assert(u.distance(u.point_along(first,24),u.point_along(next,24))<1e-9,'extension changed near guidance')
assert(u.length(next)>90,'future verified section was not published')
assert(field.cost(a.field,a.field.x0,a.field.y0)==1)
assert(corridor.cost({a},a.field.x0,a.field.y0)==math.huge,"shortcut skimmed the verified coverage edge")
local full=corridor.refine({a,b,c},a.from,next)
assert(u.length(full)<u.length(corridor.extract({a,b,c},a.from)),'cross-section shortcut was not adopted')
-- Observe the same immutable guidance at many slightly off-route positions.
-- Feeding the previous vehicle correction back into it used to create loops.
local original_count=#full
for x=1,30 do
  local position=u.point_along(full,x)
  position.y=position.y+0.25
  local route=corridor.trim(full,position)
  assert(route and #route<=original_count+2)
  assert(route[#route].x==180.5)
end
assert(#full==original_count,'consumption mutated the cached route')
local cursor
for distance=1,100 do
  local position=u.point_along(full,distance)
  local route
  route,cursor=corridor.trim(full,position,cursor)
  assert(route and u.distance(route[#route],full[#full])<1e-9,'cursor lost a consumed corner')
end
assert(not corridor.trim(full,{x=-20,y=-20}),'off-route recovery must use a field')
local wall=function(x,y) return x>=45 and x<=55 and y<10 and 2 or 0 end
local p=plan({x=30.5,y=0.5},{x=70.5,y=0.5},wall)
local route=corridor.refine({p},p.from)
assert(#route>2,'shortcut crossed an inflated wall')
assert(corridor.cost({p},50,0)==math.huge)
assert(corridor.cost({p},1000,1000)==math.huge,'unknown terrain cannot authorize shortcuts')
assert(not field.cost(p.field,1000,1000),'outside coverage must differ from blocked terrain')
-- A final target inside an obstacle's clearance (a Gleba plant beside it) is blocked in the
-- inflated grid even in the field rooted at it. Guidance near the target must still end
-- there, not fall back to an earlier section and loop back.
local tree=function(x,y) return x==97 and y==0 and 4 or 0 end
local s1=plan({x=0.5,y=0.5},{x=60.5,y=0.5},tree)
local f2=field.new({x=40.5,y=0.5},{x=96.5,y=0.5},1,false,nil,16,{smooth=true})
repeat until field.slice(f2,tree,100)
local s2={field=f2,from={x=40.5,y=0.5},target={x=96.5,y=0.5},final=true}
assert(field.cost(f2,96,0)==math.huge,'fixture: the target tile lies in the dilated tree')
local near=corridor.refine({s1,s2},{x=90.5,y=0.5},corridor.refine({s1,s2},{x=80.5,y=0.5}))
assert(u.length(near)<12,'guidance near the target looped through an earlier section: '..u.length(near))
local at=corridor.extract({s1,s2},{x=96.5,y=0.5})
assert(u.length(at)<2,'route from the target left it: '..u.length(at))
print('Progressive corridor: bounded slices, farther guidance, stable prefix, immutable consumption and safe shortcuts pass')
