data:extend({
  {type="selection-tool",name="factorio-autodrive-target",icon="__base__/graphics/icons/car.png",icon_size=64,
    flags={"only-in-cursor","spawnable"},stack_size=1,always_include_tiles=true,
    select={border_color={0.2,1,0.4},mode={"any-tile"},cursor_box_type="copy"},
    alt_select={border_color={0.2,1,0.4},mode={"any-tile"},cursor_box_type="copy"}},
  {type="shortcut",name="factorio-autodrive-target",action="lua",icon="__base__/graphics/icons/car.png",icon_size=64,
    small_icon="__base__/graphics/icons/car.png",small_icon_size=64,associated_control_input="factorio-autodrive-tool"},
  {type="custom-input",name="factorio-autodrive-tool",key_sequence="CONTROL + ALT + T",consuming="none"},
  {type="custom-input",name="factorio-autodrive-resume",key_sequence="CONTROL + ALT + D",consuming="none"}
})
for _,control in ipairs({"move-up","move-down","move-left","move-right"}) do
  data:extend({{type="custom-input",name="factorio-autodrive-override-"..control,
    key_sequence="",linked_game_control=control,consuming="none"}})
end
