-- Verified overlapping fields form a rolling corridor. Field searches remain
-- resumable; only completed sections can contribute guidance or shortcuts.
local field=require("field")
local smoothing=require("route-smoothing")
local u=require("math-util")
local M={}
local function append(out,points)
  for i=2,#points do out[#out+1]=points[i] end
end
function M.cost(plans,x,y,first)
  local result,interior=nil,false
  for i=first or 1,#plans do
    local f=plans[i].field
    local c=field.cost(f,x,y)
    -- Leave three tiles at the coverage edge for ordinary tracking error.
    -- A geometric shortcut may otherwise skim a window the tank then leaves.
    if x>=f.x0+3 and y>=f.y0+3 and x<f.x1-3 and y<f.y1-3 then interior=true end
    -- Unknown coverage is distinct from blocked coverage. At overlaps use the
    -- conservative cost, including the clearance dilation at section edges.
    if c then result=math.max(result or 1,c) end
  end
  return interior and result or math.huge
end
function M.covered(plans,route,first)
  for i=2,#route do
    if smoothing.line_cost(route[i-1],route[i],function(x,y) return M.cost(plans,x,y,first) end,
      math.huge)==math.huge then return false end
  end
  return true
end
-- Starts in the newest section that covers the position with a finite field value. The
-- position's own tile may be blocked in the inflated grid (a vehicle brushing a plant, or a
-- target beside one); field.route starts from the nearest open tile, as the simulator does.
-- Requiring an open tile here used to fall back to the first section and loop back.
function M.extract(plans,position)
  local first=1
  for i=2,#plans do
    local cost=field.cost(plans[i].field,math.floor(position.x),math.floor(position.y))
    if cost and field.sample(plans[i].field,position.x,position.y)<math.huge then first=i end
  end
  local route=field.route(plans[first].field,position)
  for i=first+1,#plans do
    append(route,field.route(plans[i].field,route[#route]))
  end
  return route
end
-- Consume only nearby segments, so a loop cannot jump to a distant crossing.
function M.trim(route,position,cursor)
  if not route or #route<2 then return nil end
  local best,segment,projection=math.huge,nil,nil
  local traveled=0
  for i=cursor or 2,#route do
    local a,b=route[i-1],route[i]
    local dx,dy=b.x-a.x,b.y-a.y
    local len2=dx*dx+dy*dy
    local t=len2>0 and u.clamp(((position.x-a.x)*dx+(position.y-a.y)*dy)/len2,0,1) or 0
    local p={x=a.x+t*dx,y=a.y+t*dy}
    local d=u.distance(position,p)
    if d<best then best,segment,projection=d,i,p end
    traveled=traveled+math.sqrt(len2)*(1-t)
    if traveled>=32 then break end
  end
  if best>3 then return nil end
  local out={{x=position.x,y=position.y}}
  if best>0.01 then out[#out+1]=projection end
  for i=segment,#route do out[#out+1]=route[i] end
  return out,segment
end
function M.refine(plans,position,previous)
  local stable=M.trim(previous,position)
  local prefix={{x=position.x,y=position.y}}
  local anchor=position
  if stable then
    -- Keep the next 24 tiles fixed while refining the distant tail.
    anchor=u.point_along(stable,24)
    local rest=24
    for i=2,#stable do
      local length=u.distance(stable[i-1],stable[i])
      if length>=rest then break end
      prefix[#prefix+1]=stable[i]
      rest=rest-length
    end
    prefix[#prefix+1]=anchor
  end
  local tail=M.extract(plans,anchor)
  if plans[1].field.smooth then tail=smoothing.route(tail,function(x,y) return M.cost(plans,x,y) end) end
  append(prefix,tail)
  return prefix
end
return M
