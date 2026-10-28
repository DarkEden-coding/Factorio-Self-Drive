-- Cost-aware arbitrary-angle shortcuts. Keep traversal/order in sync with
-- crates/fsim-core/src/route_smoothing.rs (field-smooth-v1).
local M = {}
local EPS = 1e-10
local function point_open(p,cost)
  local x,y=math.floor(p.x),math.floor(p.y)
  local on_x,on_y=math.abs(p.x-x)<EPS,math.abs(p.y-y)<EPS
  return cost(x,y)<math.huge and (not on_x or cost(x-1,y)<math.huge)
    and (not on_y or cost(x,y-1)<math.huge)
    and (not (on_x and on_y) or cost(x-1,y-1)<math.huge)
end
local function line_cost(a,b,cost,budget)
  if not point_open(a,cost) or not point_open(b,cost) then return math.huge end
  local dx,dy=b.x-a.x,b.y-a.y
  local length=math.sqrt(dx*dx+dy*dy)
  if length<EPS then return 0 end
  local x,y=math.floor(a.x),math.floor(a.y)
  local sx,sy=dx>0 and 1 or -1,dy>0 and 1 or -1
  local dt_x,dt_y=math.abs(dx)>EPS and 1/math.abs(dx) or math.huge,
    math.abs(dy)>EPS and 1/math.abs(dy) or math.huge
  local tx,ty=dx>0 and x+1-a.x or a.x-x,dy>0 and y+1-a.y or a.y-y
  tx,ty=dt_x<math.huge and tx*dt_x or math.huge,dt_y<math.huge and ty*dt_y or math.huge
  local along_x=math.abs(dx)<EPS and math.abs(a.x-math.floor(a.x))<EPS
  local along_y=math.abs(dy)<EPS and math.abs(a.y-math.floor(a.y))<EPS
  local t,total=0,0
  while t<1 do
    local cell=cost(x,y)
    if cell==math.huge or (along_x and cost(x-1,y)==math.huge)
      or (along_y and cost(x,y-1)==math.huge) then return math.huge end
    local next_t=math.min(tx,ty,1)
    total=total+(next_t-t)*length*cell
    if total>budget+1e-8 then return math.huge end
    if next_t>=1 then break end
    if math.abs(tx-ty)<EPS then
      if cost(x+sx,y)==math.huge or cost(x,y+sy)==math.huge then return math.huge end
      x,y,tx,ty=x+sx,y+sy,tx+dt_x,ty+dt_y
    elseif tx<ty then x,tx=x+sx,tx+dt_x
    else y,ty=y+sy,ty+dt_y end
    t=next_t
  end
  return total
end
-- Shared by corridor coverage checks; includes every touched corner/border cell.
M.line_cost=line_cost
function M.route(points,cost)
  if #points<3 then return points end
  local prefix,invalid={[1]=0},{[1]=0}
  for i=2,#points do
    local c=line_cost(points[i-1],points[i],cost,math.huge)
    prefix[i]=prefix[i-1]+(c<math.huge and c or 0)
    invalid[i]=invalid[i-1]+(c<math.huge and 0 or 1)
  end
  local out,anchor={points[1]},1
  while anchor+1<=#points do
    local candidate=#points
    while candidate>anchor+1 do
      local budget=prefix[candidate]-prefix[anchor]
      if invalid[candidate]==invalid[anchor] and
        line_cost(points[anchor],points[candidate],cost,budget)<math.huge then break end
      candidate=anchor+math.floor((candidate-anchor)/2)
    end
    out[#out+1]=points[candidate]
    anchor=candidate
  end
  return out
end
return M
