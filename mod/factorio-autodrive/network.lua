-- Same accumulation order as python/src/fsim_rl/student_ref.py.
local model = require("weights")
local M = {model = model}
local RAYS = model.obs.ray_count * model.obs.ray_channels_count
local ROUTE = 2 * #model.obs.route_lookaheads + 1
local STUDENT = model.obs.size
-- Zero-padded 2D conv + ReLU. x and the result are flat channel-major planes;
-- weights are flat [out][in][k][k].
local function conv2d(layer, x)
  local out, n, k, stride, pad = {}, layer.size_in, layer.kernel, layer.stride, layer.pad
  local m, w = layer.size_out, layer.weight
  for o = 0, layer.out - 1 do
    local bias = layer.bias[o + 1]
    for py = 0, m - 1 do
      for px = 0, m - 1 do
        local s = bias
        for c = 0, layer["in"] - 1 do
          local wc, xc = (o * layer["in"] + c) * k * k, c * n * n
          for ky = 0, k - 1 do
            local y = py * stride + ky - pad
            if y >= 0 and y < n then
              local wr, xr = wc + ky * k, xc + y * n
              for kx = 0, k - 1 do
                local xx = px * stride + kx - pad
                if xx >= 0 and xx < n then s = s + w[wr + kx + 1] * x[xr + xx + 1] end
              end
            end
          end
        end
        out[(o * m + py) * m + px + 1] = s > 0 and s or 0
      end
    end
  end
  return out
end
-- The dense input: rays, the route features if the model reads them, the other
-- student features, then the map (through its convs).
local function actor_input(x)
  if not model.map then return x end
  local input = {}
  for i = 1, RAYS do input[#input + 1] = x[i] end
  local from = model.route and RAYS + 1 or RAYS + ROUTE + 1
  for i = from, STUDENT do input[#input + 1] = x[i] end
  local cells = {}
  for i = STUDENT + 1, #x do cells[#cells + 1] = x[i] end
  for _, layer in ipairs(model.map.conv) do cells = conv2d(layer, cells) end
  for i = 1, #cells do input[#input + 1] = cells[i] end
  return input
end
function M.forward(features)
  local x, norm = {}, model.normalizer
  for i = 1, #features do
    local v = (features[i] - norm.mean[i]) / norm.std[i]
    x[i] = math.max(-norm.clip, math.min(norm.clip, v))
  end
  x = actor_input(x)
  for n, layer in ipairs(model.dense) do
    local out, weights, inputs = {}, layer.weight, layer["in"]
    for o = 1, layer.out do
      local s, offset = layer.bias[o], (o - 1) * inputs
      for i = 1, inputs do s = s + weights[offset + i] * x[i] end
      out[o] = n < #model.dense and math.max(0, s) or s
    end
    x = out
  end
  local best = 1
  for i = 2, #x do if x[i] > x[best] then best = i end end
  return best - 1, x
end
return M
