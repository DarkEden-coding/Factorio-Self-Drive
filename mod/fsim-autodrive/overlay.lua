local M = {}
function M.draw(s,player)
  if not s.vehicle.valid then return end
  local surface=s.vehicle.surface
  for _,mode in ipairs({"game","chart"}) do
    rendering.draw_circle({surface=surface,target=s.target,radius=1,color={0.2,1,0.4},width=2,
      time_to_live=13,players={player.index},render_mode=mode})
    for i=2,#s.route do
      rendering.draw_line({surface=surface,from=s.route[i-1],to=s.route[i],color={0.2,0.65,1},width=1,
        time_to_live=13,players={player.index},render_mode=mode})
    end
    -- The game's path (waypoints), grey under the blue route the student reads.
    local path=s.path and s.path.waypoints or {}
    for i=2,#path do
      rendering.draw_line({surface=surface,from=path[i-1],to=path[i],color={0.6,0.6,0.6,0.5},width=1,
        time_to_live=13,players={player.index},render_mode=mode})
    end
    for _,point in ipairs(s.look or {}) do
      rendering.draw_circle({surface=surface,target=point,radius=0.3,color={1,0.7,0.1},width=2,
        time_to_live=13,players={player.index},render_mode=mode})
    end
  end
end
return M
