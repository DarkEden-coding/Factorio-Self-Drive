if not bit32 then bit32={band=assert(load('return function(a,b) return a & b end'))()} end
package.path='mod/factorio-autodrive/?.lua;'..package.path
local smooth=require('route-smoothing')
local p={{x=0.5,y=0.5}}
for i=1,10 do p[#p+1]={x=i+0.5,y=i+0.5} end
for i=11,30 do p[#p+1]={x=i+0.5,y=10.5} end
local free=smooth.route(p,function() return 1 end)
assert(#free==2 and free[2].x==30.5 and free[2].y==10.5)
local detour={{x=0.5,y=0.5}}
for y=1,8 do detour[#detour+1]={x=0.5,y=y+0.5} end
for x=1,20 do detour[#detour+1]={x=x+0.5,y=8.5} end
for y=7,0,-1 do detour[#detour+1]={x=20.5,y=y+0.5} end
for _,penalty in ipairs({math.huge,10}) do
 local r=smooth.route(detour,function(x,y) return x>=8 and x<=12 and y<=5 and penalty or 1 end)
 assert(#r>2,'shortcut crossed a wall or expensive zone')
end
local corner=smooth.route({{x=0.5,y=0.5},{x=0.5,y=1.5},{x=1.5,y=1.5}},
 function(x,y) return x==1 and y==0 and math.huge or 1 end)
assert(#corner==3,'diagonal shortcut cut a blocked corner')
print('Any-angle smoothing: open routes collapse; blocked corners and expensive zones remain protected')
