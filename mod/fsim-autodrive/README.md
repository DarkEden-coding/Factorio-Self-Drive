# Fsim autodrive

Autodrive for base cars and tanks in Factorio 2.1.23 or newer in the 2.1 series.
Route planning and driving run locally in the mod. No external program is needed.

## Driving

1. Sit in the driver's seat of a car or tank and supply fuel.
2. Click **Set autodrive target** or press **Ctrl+Alt+T**.
3. Click or select a destination in the world or on the map.
4. Wait for the route to prepare. The vehicle brakes while planning, then drives.

Movement keys return manual control. **Ctrl+Alt+D** resumes toward the last target.
Selecting another target starts a new trip. Leaving the vehicle cancels autodrive.
Use `/autodrive <x> <y>` for exact coordinates or `/autodrive stop` to stop driving.

Targets must be on your surface, in generated terrain, and within 4096 tiles.
Blocked targets snap to drivable ground within 8 tiles.

## Settings

Diagnostic lines are hidden by default. Enable **Show autodrive diagnostic lines**
under **Settings > Mod settings > Per player** to display the target ring, verified
route, coarse path and lookahead points in the world and on the map.
Existing saves retain a player's previously selected setting.

**Write autodrive Lua timings** enables local profiling output. It is disabled by
default. `/autodrive-dump` writes a diagnostic snapshot during a drive.

## Limitations

The controller was trained for base cars and tanks on Nauvis terrain. Modified
vehicles, player structures, moving units, equipment and quality or research
bonuses are outside its trained driving domain. Watch the vehicle and take over
if needed. A stalled or timed-out trip brakes until manual input or a new target.

Updating the mod clears active trips. Set a target again after loading the update.
