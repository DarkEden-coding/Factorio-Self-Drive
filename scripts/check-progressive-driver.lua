-- Production planner/cache integration with a stationary vehicle adapter. Engine
-- suites cover real controls/physics; this checks a deliberately delayed worker.
if not bit32 then
  bit32={band=assert(load('return function(a,b) return a & b end'))(),
    bor=assert(load('return function(a,b) return a | b end'))()}
end
-- Any defines.<group>.<name> reads as "<group>.<name>" (trains.lua indexes several at load).
defines=defines or setmetatable({},{__index=function(t,group)
  local sub=setmetatable({},{__index=function(_,name) return group.."."..name end})
  rawset(t,group,sub); return sub end})
package.path='mod/factorio-autodrive/?.lua;'..package.path
game={tick=0}
prototypes={entity={car={collision_mask={layers={player=true,train=true,car=true,is_object=true}}}}}
helpers={create_profiler=function() return {stop=function() end} end}
local stats={half_width=0.7,half_length=1,tank_driving=false,weight=700,max_health=450,
  rotation_speed=0.015,braking_power=200000,power=150000,top_speed=0.4,
  fuel_acceleration=1,fuel_top_speed=1,tile_friction=1}
-- A forward ray at exact integer coordinates must see a two-tile-away wall
-- for every cardinal heading, including parallel axes with signed roundoff.
local raster,observation=require('raster'),require('observation')
local tiles={}
for i=1,256 do tiles[i]=false end
for _,p in ipairs({{10,8},{5,8},{8,10},{8,5}}) do tiles[p[2]*16+p[1]+1]=true end
local scene={blocks={['0:0']=raster.build(0,0,tiles,{})}}
for _,heading in ipairs({0,0.25,0.5,0.75}) do
  local obs=observation.build(scene,{{x=8,y=8},{x=12,y=8}},
    {x=8,y=8,orientation=heading,speed=0,health=450,target={x=12,y=8},elapsed=0,stall=0,time_limit=600},stats)
  assert(math.abs(obs[16]-1/32)<1e-7,'parallel forward ray missed a wall')
end
package.loaded.vehicle={stats=function() return stats end,
  brake=function(v) v.braked=true end,apply=function(v) v.braked=false end}
local grid=require('grid')
local real_slice=grid.slice
local delayed=false
grid.slice=function(...) if not delayed then return real_slice(...) end end
local driver,u=require('driver'),require('math-util')
local v={valid=true,name='car',position={x=0.5,y=0.5},orientation=0.25,speed=0,health=450,
  prototype={collision_box={},collision_mask={}},force={}}
v.surface={is_chunk_generated=function() return true end,request_path=function() return 1 end,
  get_tile=function() return {valid=true,name='grass-1',prototype={name='grass-1',vehicle_friction_modifier=1.6,
    collision_mask={layers={ground_tile=true}}}} end,
  get_territory_for_chunk=function() return nil end,
  find_entities_filtered=function() return {} end,count_entities_filtered=function() return 0 end}
local s=driver.start(v,{x=1000.5,y=0.5})
local function tick()
  game.tick=game.tick+1
  local result=driver.tick(s)
  assert(not result,'unexpected terminal state: '..tostring(result))
end
repeat tick() until s.status=='driving'
local target=s.target
v.position=u.point_along(s.guidance,40)
v.speed=0.4
delayed=true
for _=1,8 do tick() end
assert(s.frontier_wait_ticks>0 and v.braked,'unverified frontier did not brake')
local elapsed=s.elapsed
delayed=false
for _=1,300 do
  tick()
  if #s.corridor>1 and not v.braked then break end
end
assert(#s.corridor>1 and not v.braked,'ready extension did not resume control')
assert(s.elapsed>elapsed and s.target==target,'extension reset the trip clock or destination')
assert(u.length(s.route)>100 and s.max_cached_sections>1,'verified horizon did not grow')
local old=s.corridor[1]
local far=s.corridor[#s.corridor]
driver.dirty(s,far.target)
assert(s.repair_index and s.corridor[1]==old,'invalidation discarded unaffected coverage')
for _=1,300 do tick(); if not s.repair_index then break end end
assert(not s.repair_index,'dirty coverage was not rebuilt')
assert(s.target==target and s.elapsed>elapsed,'repair reset the destination or trip clock')
print('Progressive driver: frontier braking, cached extension, continuous trip state and selective repair pass')
-- Escape rule (same sequence as python/tests/test_escape.py): reversing left into a cliff
-- end without moving switches to forward right for HOLD decisions, then hands back.
local escape=require('escape')
local e,executed=escape.new(),{}
for i=1,12 do
  local position=i<=10 and {x=5,y=5} or {x=5,y=5+i}
  executed[i]=escape.decide(e,position,0)
end
assert(table.concat(executed,',')=='0,0,0,0,0,0,0,8,8,8,8,0','escape sequence: '..table.concat(executed,','))
print('Escape: blocked pushing triggers the mirrored maneuver for its hold, then hands back')
