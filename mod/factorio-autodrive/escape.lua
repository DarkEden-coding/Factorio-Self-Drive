-- Escape rule for blocked poses. A vehicle pressed against a cliff end or rock can map to an
-- action that does not move it, and the argmax policy then repeats it until the stall limit.
-- After AFTER decisions that all pushed (throttle not coasting) and each moved the vehicle
-- less than MOVED tiles, the next HOLD decisions use the opposite throttle and opposite
-- steering of the blocked action. Same rule as python/src/fsim_rl/escape.py.
local M={AFTER=6,HOLD=4,MOVED=0.02}
function M.new() return {blocked=0,left=0,action=4,last=4,started=false} end
function M.mirror(action)
  local throttle,steer=math.floor(action/3)-1,action%3-1
  return (1-throttle)*3+(1-steer)
end
-- Called once per decision with the vehicle position and the policy's action; returns the
-- action to execute and whether the rule chose it.
function M.decide(state,position,action)
  if state.x then
    local moved=math.sqrt((position.x-state.x)^2+(position.y-state.y)^2)
    if math.floor(state.last/3)~=1 and moved<M.MOVED then state.blocked=state.blocked+1 else state.blocked=0 end
  end
  -- The first decision's movement is not counted, as in the simulator, which has no pose
  -- before the first decision.
  if state.started then state.x,state.y=position.x,position.y end
  state.started=true
  if state.blocked>=M.AFTER and state.left==0 then state.action,state.left,state.blocked=M.mirror(state.last),M.HOLD,0 end
  local escaping=state.left>0
  if escaping then action,state.left=state.action,state.left-1 end
  state.last=action
  return action,escaping
end
return M
