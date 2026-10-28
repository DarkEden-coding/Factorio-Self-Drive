# Factorio Autodrive

Pick a destination and let your car or tank do the driving!

Factorio Autodrive uses a trained reinforcement learning model to steer and
control the throttle along dynamic paths. The route planner extends and refines
the path as you travel, while the model follows it toward your destination.
Everything runs right inside Factorio, with no external setup needed.

## Take it for a spin

1. Hop into the driver's seat of a car or tank and add some fuel.
2. Click **Set autodrive target** or press **Ctrl+Alt+T**.
3. Pick a destination in the world or on the map. Give it a moment to plan, then off you go!

Use the movement keys to take over anytime. Press **Ctrl+Alt+D** to resume, or
pick a new destination to start another trip.

## Want to see the path?

Diagnostic lines are off by default. Turn on **Show autodrive diagnostic lines**
in **Settings > Mod settings > Per player** to see the route and the points the
model is following.

## The basics

- Supports base cars and tanks on Factorio 2.1.23 or newer in the 2.1 series.
- Destinations must be on the same surface, in generated terrain, and within 4096 tiles.
- Trained for driving on Nauvis. Modded vehicles and busy factory layouts can trip it up,
  so keep an eye on the ride and take the wheel when needed.
