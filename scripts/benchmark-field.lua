-- Compare two field implementations on identical masks. Run from the repo root:
-- lua scripts/benchmark-field.lua BEFORE.lua mod/factorio-autodrive/field.lua
if not bit32 then bit32={band=assert(load('return function(a,b) return a & b end'))()} end
package.path='mod/factorio-autodrive/?.lua;'..package.path
local before, after = dofile(arg[1]), dofile(arg[2])
local function mask(x,y)
  if (x-60)^2+(y-20)^2<400 then return 17 end
  if (x+40)^2+(y+50)^2<324 then return 32 end
  if ((x*73856093 ~ y*19349663) & 0xffff)%100<3 then return 5 end
  return 0
end
for _, d in ipairs({48,160,256}) do
  local previous, baseline
  for n, module in ipairs({before,after}) do
    local times, ticks, field = {},0,nil
    for repeat_i=1,7 do
      collectgarbage('collect')
      field=module.new({x=0.5,y=0.5},{x=d*0.8,y=d*0.6},0.7,false,nil,48)
      local start=os.clock()
      ticks=1
      while not module.slice(field,mask,4000) do ticks=ticks+1 end
      times[#times+1]=(os.clock()-start)*1000
    end
    table.sort(times)
    if previous then
      for i,v in ipairs(field.values) do assert(v==previous[i], 'field changed at cell '..i) end
      print(string.format('distance=%d cells=%d before_ms=%.3f after_ms=%.3f speedup=%.2fx slices=%d',
        d,field.w*field.h,baseline,times[4],baseline/times[4],ticks))
    else previous,baseline=field.values,times[4] end
  end
end
