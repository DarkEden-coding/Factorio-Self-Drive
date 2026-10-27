#!/usr/bin/env python3
"""Extract the selected 2.1.23 base Lua without executing game code."""
import argparse
import ast
import json
import math
import operator
from pathlib import Path
import re

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("root", type=Path, help="Factorio Contents, data, or data/base directory")
parser.add_argument("-o", "--output", type=Path, default=Path("data/prototypes.json"))
options = parser.parse_args()
root = options.root.expanduser().resolve()
if (root / "data/base").is_dir():
    root = root / "data"
elif root.name == "base" and (root / "prototypes").is_dir():
    root = root.parent
api_path = root.parent / "doc-html/prototype-api.json"
api = json.loads(api_path.read_text())
version = api["application_version"]
if version != "2.1.23":
    parser.error(f"text extractor supports 2.1.23, found {version}")
car_api = next(p for p in api["prototypes"] if p["name"] == "CarPrototype")
immunity_defaults = {p["name"]: p["default"]["value"] for p in car_api["properties"]
                     if p["name"].startswith("immune_to_")}

def read(path):
    return (root / path).read_text()

def enclosing_table(text, position):
    tokens = re.finditer(r'--\[\[.*?\]\]|--[^\n]*|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|[{}]', text, re.S)
    stack = []
    start = None
    for token in tokens:
        if token[0] == '{':
            stack.append(token.start())
        elif token[0] == '}':
            opening = stack.pop()
            if start == opening:
                return text[start:token.end()]
        if start is None and token.end() >= position:
            start = stack[-1]
    raise ValueError("unclosed prototype table")


def section(text, name):
    return enclosing_table(text, text.index('name = "' + name + '"'))


def vector(text, key, variables=None):
    match = re.search(r'\b' + key + r'\s*=\s*\{([^}]+)\}', text)
    expressions = match[1].split(',')
    for name, value in (variables or {}).items():
        expressions = [re.sub(r'\b' + name + r'\b', str(value), x) for x in expressions]
    values = [arithmetic(x.strip()) for x in expressions]
    return [int(value) if value.is_integer() else value for value in values]

def arithmetic(expression):
    """Only numeric literals and base Lua arithmetic, never eval Lua/Python code."""
    operators = {ast.Add: operator.add, ast.Sub: operator.sub,
                 ast.Mult: operator.mul, ast.Div: operator.truediv}
    def visit(node):
        if isinstance(node, ast.Constant) and type(node.value) in (int, float):
            return node.value
        if isinstance(node, ast.BinOp) and type(node.op) in operators:
            return operators[type(node.op)](visit(node.left), visit(node.right))
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
            return -visit(node.operand)
        raise ValueError(f"unsupported numeric expression: {expression}")
    return float(visit(ast.parse(expression, mode="eval").body))


def number(text, key, default=None):
    m = re.search(r'\b' + re.escape(key) + r'\s*=\s*([^,\n}]+)', text)
    return arithmetic(m[1].strip()) if m else default


def boolean(text, key, default):
    m = re.search(r'\b' + re.escape(key) + r'\s*=\s*(true|false)\b', text)
    return m[1] == 'true' if m else default

def power(text, key):
    m = re.search(r'\b' + key + r'\s*=\s*"([\d.]+)([kMG]?)[WJ]"', text)
    return float(m[1]) * {'': 1, 'k': 1e3, 'M': 1e6, 'G': 1e9}[m[2]]

def box(text):
    m = re.search(r'collision_box\s*=\s*\{\{([^}]+)\},\s*\{([^}]+)\}\}', text)
    return [[float(x) for x in m[i].split(',')] for i in (1, 2)]

def resistances(text):
    pos = text.find('resistances =')
    if pos < 0: return []
    s = text[pos:]
    start = s.index('{'); depth = 0; end = 0
    for i in range(start, len(s)):
        depth += (s[i] == '{') - (s[i] == '}')
        if depth == 0: end = i + 1; break
    return [dict(type=m[1], decrease=number(m[2], 'decrease', 0), percent=number(m[2], 'percent', 0))
            for m in re.finditer(r'type\s*=\s*"([^"]+)"([^}]+)', s[start:end])]

def color(text):
    s = re.search(r'map_color\s*=\s*\{([^}]+)\}', text)[1]
    nums = [float(x) for x in re.findall(r'[\d.]+', s)]
    return [int(math.floor(x * (255 if max(nums) <= 1 else 1) + .5)) for x in nums[:3]]

entities_lua = read('base/prototypes/entity/entities.lua')
vehicles = []
for name in ['car', 'tank']:
    s = section(entities_lua, name)
    v = dict(name=name, weight=number(s, 'weight'), braking_power=number(s, 'braking_force') * 60,
             consumption=power(s, 'consumption'), effectivity=number(s, 'effectivity'),
             friction=number(s, 'friction_force'), rotation_speed=number(s, 'rotation_speed'),
             rotation_snap_angle=number(s, 'rotation_snap_angle'), max_health=number(s, 'max_health'),
             collision_box=box(s), terrain_friction_modifier=number(s, 'terrain_friction_modifier', 1),
             energy_per_hit_point=number(s, 'energy_per_hit_point'), resistances=resistances(s),
             tank_driving='tank_driving = true' in s,
             **{key: boolean(s, key, default) for key, default in immunity_defaults.items()})
    vehicles.append(v)
fuels = []
items = read('base/prototypes/item.lua')
for name in ['wood', 'coal', 'solid-fuel', 'rocket-fuel', 'nuclear-fuel']:
    s = section(items, name)
    fuels.append(dict(name=name, fuel_value=power(s, 'fuel_value'),
                      fuel_acceleration_multiplier=number(s, 'fuel_acceleration_multiplier', 1),
                      fuel_top_speed_multiplier=number(s, 'fuel_top_speed_multiplier', 1)))
tiles = []
tile_lua = read('base/prototypes/tile/tiles.lua')
masks = read('base/prototypes/tile/tile-collision-masks.lua')
names = ['grass-'+str(i) for i in range(1,5)] + ['dry-dirt'] + ['dirt-'+str(i) for i in range(1,8)] + ['red-desert-'+str(i) for i in range(4)] + ['sand-'+str(i) for i in range(1,4)] + ['water','deepwater']
for i, name in enumerate(names):
    s = section(tile_lua, name)
    m = re.search(r'vehicle_friction_modifier\s*=\s*(\w+)', s)
    friction = number(tile_lua, m[1]) if m else 1
    water = name in ['water','deepwater']
    mask_name = re.search(r'collision_mask\s*=\s*tile_collision_masks\.(\w+)\(\)', s)[1]
    mask = masks.split('tile_collision_masks.' + mask_name + ' = function()', 1)[1].split('end', 1)[0]
    layers = re.findall(r'(\w+)\s*=\s*true', re.sub(r'--[^\n]*', '', mask))
    tiles.append(dict(id=i, name=name, color=color(s), vehicle_friction_modifier=friction,
                      is_water=water, collision_mask=layers))
obstacles = []
trees_lua = read('base/prototypes/entity/trees.lua')
s = enclosing_table(trees_lua, trees_lua.index('name = name,'))
for name in ['tree-01','tree-02','tree-04','tree-06','tree-08','tree-09']:
    obstacles.append(dict(name=name, kind='tree', max_health=number(s, 'max_health'), collision_box=box(s), resistances=resistances(s)))
for name in ['dry-tree','dead-tree-desert']:
    s = section(trees_lua,name)
    obstacles.append(dict(name=name,kind='tree',max_health=number(s,'max_health'),collision_box=box(s),resistances=resistances(s)))
rocks = read('base/prototypes/decorative/decoratives.lua')
for name in ['big-rock','huge-rock','big-sand-rock']:
    s = section(rocks,name)
    obstacles.append(dict(name=name,kind='rock',max_health=number(s,'max_health'),collision_box=box(s),resistances=resistances(s)))
s = section(read('base/prototypes/entity/enemies.lua'),'biter-spawner')
obstacles.append(dict(name='biter-spawner',kind='nest',max_health=number(s,'max_health'),collision_box=box(s),resistances=resistances(s)))
orientations = {}
helper = read('base/prototypes/entity/entity-util.lua')
for line in helper.splitlines():
    if '= cliff_orientation(sprite_params,' not in line: continue
    name = line.strip().split()[0].replace('_','-')
    if 'rotbb(' in line:
        args = [float(x) for x in re.search(r'rotbb\(([^)]+)\)',line)[1].split(',')]
        x,y,size,intersect = args
        dist = size/2 * 1.4142135623730951
        dx = (1-intersect/size)*dist; dy = intersect/size*dist
        cx=x+size/2;cy=y+size/2
        b=[[cx-dx,cy-dy],[cx+dx,cy+dy]];orientation=.125
    else:
        m = re.search(r'\{\{([^}]+)\},\s*\{([^}]+)\}\}',line)
        b=[[float(x) for x in m[i].split(',')] for i in (1,2)];orientation=0
    orientations[name]=dict(collision_box=b,orientation=orientation)
cliff_section = section(entities_lua, 'cliff')
scale = number(cliff_section, 'scale')
scaled_cliff = helper[helper.index('function scaled_cliff('):]
grid_size = vector(scaled_cliff, 'grid_size', {'scale': scale})
if scale != 1:
    raise ValueError('only the normal-scale base cliff is supported')
grid_offset = vector(scaled_cliff, 'grid_offset')
resources = []
r = read('base/prototypes/entity/resources.lua')
for i,name in enumerate(['iron-ore','copper-ore','coal','stone','uranium-ore'],1):
    resources.append(dict(id=i,name=name,color=color(section(r,name))))
result=dict(schema_version=2, source=dict(game_version=version, kind='installed-game'),
            vehicles=vehicles,fuels=fuels,tiles=tiles,entities=obstacles,
            cliffs=dict(name='cliff',max_health=None,grid_size=grid_size,grid_offset=grid_offset,orientations=orientations),resources=resources,
            physics=dict(reverse_power_multiplier=.5,car_steering_full_speed=.1,stop_speed=.00001,
                         collision_energy_scale=1,collision_retained_energy=.9,max_translation_substep=.2,max_rotation_substep=.002,
                         nest_penalty_radius=18,target_radius=3))
options.output.write_text(json.dumps(result,indent=2)+'\n')
print('Wrote', len(tiles),'tiles,',len(obstacles),'entities,',len(orientations),'cliff orientations')
