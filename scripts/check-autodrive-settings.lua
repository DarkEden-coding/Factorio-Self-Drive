-- Exercise production event wiring with per-player settings and rendering stubs.
local defaults={}
data={extend=function(_,entries)
  for _,entry in ipairs(entries) do defaults[entry.name]=entry.default_value end
end}
dofile('mod/factorio-autodrive/settings.lua')
assert(defaults['factorio-autodrive-overlay']==false,'diagnostics must default off')
local enabled=defaults['factorio-autodrive-overlay']
local draws=0
package.path='mod/factorio-autodrive/?.lua;'..package.path
package.loaded.driver={tick=function() end}
package.loaded.vehicle={}
rendering={draw_line=function() draws=draws+1 end,draw_circle=function() draws=draws+1 end}
local player={index=1,connected=true,character={}}
local vehicle={valid=true,name='car',surface={},get_driver=function() return player end}
player.physical_vehicle=vehicle
local state={vehicle=vehicle,target={x=2,y=2},route={{x=0,y=0},{x=2,y=2}}}
storage={autodrive={[1]=state}}
game={tick=12,get_player=function() return player end}
settings={get_player_settings=function() return {['factorio-autodrive-overlay']={value=enabled}} end}
local handlers={}
defines=setmetatable({events=setmetatable({},{__index=function(_,name) return name end})},
  {__index=function(t,group)
    local sub=setmetatable({},{__index=function(_,name) return group.."."..name end})
    rawset(t,group,sub); return sub end})
script={on_init=function() end,on_configuration_changed=function() end,
  on_event=function(event,handler) handlers[event]=handler end}
commands={add_command=function() end}
remote={add_interface=function() error('production must not register a test interface') end}
dofile('mod/factorio-autodrive/control.lua')
handlers.on_tick()
assert(draws==0,'default setting drew diagnostics')
enabled=true
handlers.on_tick()
assert(draws>0,'enabling diagnostics did not render the overlay')
local previous=draws
enabled=false
handlers.on_tick()
assert(draws==previous,'disabling diagnostics kept rendering the overlay')
print('Autodrive settings: hidden by default, enable and disable passed')
