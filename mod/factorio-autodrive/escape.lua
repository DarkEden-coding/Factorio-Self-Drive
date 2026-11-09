-- Escape rule for blocked poses. A vehicle pressed against a rock or cliff can map to an
-- action that does not move it, and the argmax policy then repeats it until the stall limit.
-- After AFTER decisions that all pushed (throttle not coasting) and each moved the vehicle
-- less than MOVED tiles, the next HOLD decisions use a maneuver, trying in turn: the same
-- throttle straight, the opposite throttle straight, the opposite throttle and steering, and
-- the opposite throttle with the same steering. A maneuver that does not move the vehicle
-- hands straight to the next; any movement resets the sequence.
-- Same rule as python/src/fsim_rl/escape.py.
local M={AFTER=6,HOLD=4,MOVED=0.02}
function M.new() return {blocked=0,left=0,action=4,last=4,base=4,attempt=0,started=false} end
function M.maneuvers(action)
  local throttle,steer=math.floor(action/3)-1,action%3-1
  local out={}
  if steer~=0 then out[#out+1]={throttle,0} end
  out[#out+1]={-throttle,0}
  out[#out+1]={-throttle,steer~=0 and -steer or -1}
  out[#out+1]={-throttle,steer~=0 and steer or 1}
  for i,m in ipairs(out) do out[i]=(m[1]+1)*3+m[2]+1 end
  return out
end
-- Called once per decision with the vehicle position and the policy's action; returns the
-- action to execute and whether the rule chose it.
function M.decide(state,position,action)
  if state.x then
    local moved=math.sqrt((position.x-state.x)^2+(position.y-state.y)^2)>=M.MOVED
    if math.floor(state.last/3)~=1 and not moved then state.blocked=state.blocked+1 else state.blocked=0 end
    if moved then state.attempt=0 end
  end
  -- The first decision's movement is not counted, as in the simulator, which has no pose
  -- before the first decision.
  if state.started then state.x,state.y=position.x,position.y end
  state.started=true
  if state.left==0 and (state.blocked>=M.AFTER or (state.attempt>0 and state.blocked>=M.HOLD)) then
    if state.attempt==0 then state.base=state.last end
    local options=M.maneuvers(state.base)
    state.action=options[state.attempt%#options+1]
    state.attempt,state.left,state.blocked=state.attempt+1,M.HOLD,0
  end
  local escaping=state.left>0
  if escaping then action,state.left=state.action,state.left-1 end
  state.last=action
  return action,escaping
end
return M
