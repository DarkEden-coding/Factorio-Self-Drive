#!/usr/bin/env python3
"""Check fsim mods' API names against an installed Factorio's JSON definitions.

This is a name/record-field audit, not a Lua type checker or engine test. Bindings
below describe the mod's local handle aliases. New aliases must be added here.
Names themselves are extracted from Lua, including indexed vehicle properties,
map-gen tables, method parameter tables, prototype types and riding enum values.
"""
import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
TOKEN = re.compile(r'--[^\n]*|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|[A-Za-z_]\w*|\d+(?:\.\d+)?|[^\s]')


def tokens(source):
    return [m[0] for m in TOKEN.finditer(source) if not m[0].startswith('--')]


def closing(ts, start):
    pairs = {'{': '}', '(': ')', '[': ']'}
    stack = []
    for i in range(start, len(ts)):
        if ts[i] in pairs:
            stack.append(pairs[ts[i]])
        elif stack and ts[i] == stack[-1]:
            stack.pop()
            if not stack:
                return i
    raise ValueError('unclosed Lua expression')


def fields(ts, start):
    """Named fields in this table only; nested constructors are checked separately."""
    end = closing(ts, start)
    result = {}
    i = start + 1
    while i < end:
        if re.fullmatch(r'[A-Za-z_]\w*', ts[i]) and ts[i + 1] == '=':
            result[ts[i]] = i + 2
            i += 2
        if ts[i] in ('{', '(', '['):
            i = closing(ts, i)
        i += 1
    return result


class Audit:
    def __init__(self, runtime, prototype):
        self.runtime = runtime
        self.definitions = {
            'runtime': {x['name']: x for k in ('classes', 'concepts', 'events') for x in runtime[k]},
            'prototype': {x['name']: x for k in ('prototypes', 'types') for x in prototype[k]},
        }
        self.used = set()

    def members(self, stage, owner):
        if isinstance(owner, dict):
            if owner['complex_type'] == 'type':
                return self.members(stage, owner['value'])
            if owner['complex_type'] == 'union':
                return {k: v for option in owner['options'] for k, v in self.members(stage, option).items()}
            return {x['name']: x for x in owner.get('parameters', [])}
        definition = self.definitions[stage].get(owner, {})
        result = self.members(stage, definition['parent']) if definition.get('parent') else {}
        if 'type' in definition:
            result.update(self.members(stage, definition['type']))
        for key in ('attributes', 'methods', 'properties', 'parameters', 'data'):
            result.update({x['name']: x for x in definition.get(key, [])})
        return result

    def check(self, stage, owner, member):
        if member not in self.members(stage, owner):
            raise ValueError(f'{stage} {owner}.{member}: absent from API JSON')
        self.used.add((stage, owner, member))

    def record(self, ts, start, owner):
        members = self.members('runtime', owner)
        for name, value in fields(ts, start).items():
            self.check('runtime', owner, name)
            kind = members[name].get('type', members[name].get('read_type'))
            if ts[value] == '{' and isinstance(kind, str) and self.members('runtime', kind):
                self.record(ts, value, kind)
            elif ts[value] == '{' and isinstance(kind, dict) and kind['complex_type'] == 'dictionary':
                keys = kind['key']
                allowed = {x['value'] for x in keys.get('options', [])} if isinstance(keys, dict) else None
                for key, nested in fields(ts, value).items():
                    if allowed is not None and key not in allowed:
                        raise ValueError(f'{owner}.{name}: invalid dictionary key {key}')
                    self.record(ts, nested, kind['value'])

    def define(self, path):
        entries = self.runtime['defines']
        for name in path:
            node = next((x for x in entries if x['name'] == name), None)
            if node is None:
                raise ValueError('unknown defines.' + '.'.join(path))
            entries = node.get('subkeys', node.get('values', []))
        self.used.add(('runtime', 'defines', '.'.join(path)))

    def file(self, path):
        ts = tokens(path.read_text())
        autodrive = path.parent.name == 'fsim-autodrive'
        # Event callbacks in autodrive deliberately share helpers. Validate the
        # union of documented event fields; individual callback wiring is also
        # exercised by its isolated engine harness.
        if autodrive:
            events = ['CustomCommandData', 'CustomInputEvent', 'on_lua_shortcut',
                      'on_player_selected_area', 'on_entity_died', 'on_player_mined_entity',
                      'on_script_path_request_finished']
            self.definitions['runtime']['AutodriveEvent'] = {'parameters': list({
                k: v for event in events for k, v in self.members('runtime', event).items()}.values())}
        # Explicit handle aliases; ordinary scenario/job/output tables are excluded.
        aliases = {'player': 'LuaPlayer', 'surface': 'LuaSurface', 'job.surface': 'LuaSurface',
                   'vehicle': 'LuaEntity', 'entity': 'LuaEntity', 'e': 'LuaEntity',
                   'job.vehicle': 'LuaEntity', 'job.driver': 'LuaEntity',
                   'job.vehicle.grid': 'LuaEquipmentGrid', 'job.force': 'LuaForce',
                   'technology': 'LuaTechnology',
                   'inventory': 'LuaInventory', 'tile': 'LuaTile'}
        aliases.update({x['name']: x['type'] for x in self.runtime['global_objects']})
        if path.name == 'normalize.lua':
            aliases.update(p='MapPosition', b='BoundingBox', c='Color')
        elif path.name == 'map-export.lua':
            aliases['p'] = 'MapPosition'
            aliases['entity.prototype'] = 'LuaEntityPrototype'
            aliases['surface.map_gen_settings'] = 'MapGenSettings'
        elif path.name == 'control.lua':
            aliases['event'] = 'CustomCommandData'
        elif path.name == 'recorder.lua':
            aliases['event'] = 'on_entity_died'
        elif path.name == 'prototype-export.lua':
            aliases.update(p='LuaEntityPrototype', r='Resistance')
            aliases['p.collision_mask'] = 'TileCollisionMask'
        if autodrive:
            aliases.update({'v': 'LuaEntity', 'vehicle.surface': 'LuaSurface', 'v.surface': 'LuaSurface',
                'entity.prototype': 'LuaEntityPrototype', 'v.prototype': 'LuaEntityPrototype',
                'tile.prototype': 'LuaTilePrototype',
                'burner': 'LuaBurner', 'stack': 'LuaItemStack', 'fuel': 'LuaItemPrototype',
                'profiler': 'LuaProfiler', 'event': 'AutodriveEvent',
                'player.cursor_stack': 'LuaItemStack'})
        if autodrive and path.name in ('raster.lua', 'field.lua', 'observation.lua', 'network.lua', 'math-util.lua', 'weights.lua', 'geometry-data.lua'):
            aliases = {}
        stage = 'prototype' if path.name == 'data-final-fixes.lua' or (autodrive and path.name in ('data.lua', 'settings.lua')) else 'runtime' 
        if stage == 'prototype' and not autodrive:
            for i in range(len(ts) - 5):
                if ts[i:i + 6] == ['data', ':', 'extend', '(', '{', '{']:
                    for name in fields(ts, i + 5):
                        self.check('prototype', 'ModData', name)
            aliases = {'p': 'CarPrototype', 'orientation': 'OrientedCliffPrototype', 'b': 'BoundingBox',
                       'tile': 'TilePrototype', 'tree': 'TreePrototype', 'rock': 'SimpleEntityPrototype',
                       'nest': 'EnemySpawnerPrototype', 'cliff': 'CliffPrototype',
                       'resource': 'ResourceEntityPrototype', 'car': 'CarPrototype', 'nest.graphics_set': 'EnemySpawnerGraphicsSet'}
        if autodrive and stage == 'prototype':
            owners = {'selection-tool': 'SelectionToolPrototype', 'shortcut': 'ShortcutPrototype',
                      'custom-input': 'CustomInputPrototype'}
            for i, token in enumerate(ts):
                if token != '{':
                    continue
                record = fields(ts, i)
                if 'type' in record:
                    kind = ts[record['type']].strip('"')
                    if kind == 'bool-setting':
                        # Settings prototypes are absent from Factorio's JSON APIs.
                        # Check the small boolean schema explicitly, without
                        # claiming those fields were audited against API JSON.
                        allowed = {'type', 'name', 'setting_type', 'default_value', 'order'}
                        if set(record) - allowed:
                            raise ValueError('unknown bool-setting field')
                    if kind in owners:
                        owner = owners[kind]
                        for name, value in record.items():
                            self.check('prototype', owner, name)
                            if name in ('select', 'alt_select'):
                                for nested in fields(ts, value):
                                    self.check('prototype', 'SelectionModeData', nested)
                if 'linked_game_control' in record:
                    # Variable linked controls are expanded from this literal list.
                    for control in re.findall(r'"(move-[a-z]+)"', path.read_text()):
                        allowed = self.definitions['prototype']['LinkedGameControl']['type']['options']
                        if control not in {v['value'] for v in allowed}:
                            raise ValueError('unknown linked game control ' + control)
            return ts
        for i, token in enumerate(ts):
            # Rebindings of p in the two prototype readers, in source order.
            if ts[i:i + 3] == ['local', 'p', '=']:
                assignment = ''.join(ts[i:i + 14])
                for text, owner in [('prototypes.entity', 'LuaEntityPrototype'),
                                    ('prototypes.item', 'LuaItemPrototype'),
                                    ('prototypes.tile', 'LuaTilePrototype'),
                                    ('data.raw.car', 'CarPrototype'), ('data.raw.cliff', 'CliffPrototype')]:
                    if text in assignment:
                        aliases['p'] = owner
            is_root = not (i and ts[i - 1] in ('.', ':'))
            j = i
            parts = [token]
            while is_root and j + 2 < len(ts) and ts[j + 1] == '.' and re.fullmatch(r'[A-Za-z_]\w*', ts[j + 2]):
                parts.append(ts[j + 2])
                j += 2
            if token == 'defines' and len(parts) > 1:
                self.define(parts[1:])
            for n in range(len(parts) - 1, 0, -1):
                receiver = '.'.join(parts[:n])
                if receiver in aliases:
                    self.check(stage, aliases[receiver], parts[n])
            # Reads after an index, such as prototypes.item[s.fuel].stack_size.
            if token == 'prototypes' and ts[i + 1:i + 2] == ['.'] and i + 3 < len(ts) and ts[i + 3] == '[':
                owner = {'entity': 'LuaEntityPrototype', 'item': 'LuaItemPrototype',
                         'tile': 'LuaTilePrototype', 'mod_data': 'LuaModData'}.get(ts[i + 2])
                end = closing(ts, i + 3)
                if owner and ts[end + 1:end + 2] == ['.']:
                    self.check('runtime', owner, ts[end + 2])
            # API record parameters in method calls and riding_state assignments.
            if token in ('create_surface', 'create_entity', 'find_entities_filtered',
                         'find_tiles_filtered', 'destroy_decoratives', 'insert', 'riding_state', 'draw_circle', 'draw_line', 'set_stack'):
                if ts[i + 1:i + 2] == ['(']:
                    end = closing(ts, i + 1)
                    tables = [k for k in range(i + 2, end) if ts[k] == '{']
                elif ts[i + 1:i + 3] == ['=', '{']:
                    tables = [i + 2]
                else:
                    continue
                if tables:
                    owner = {'create_surface': 'MapGenSettings', 'find_entities_filtered': 'EntitySearchFilters',
                             'find_tiles_filtered': 'TileSearchFilters', 'insert': 'ItemStackIdentification',
                             'riding_state': 'RidingState', 'set_stack': 'ItemStackIdentification'}.get(token)
                    if owner:
                        self.record(ts, tables[0], owner)
                    else:
                        for name in fields(ts, tables[0]):
                            owner = 'LuaRendering' if token in ('draw_circle', 'draw_line') else 'LuaSurface'
                            method = self.members('runtime', owner)[token]
                            parameters = method['parameters'] + [p for group in method.get('variant_parameter_groups', [])
                                                                   for p in group['parameters']]
                            if name not in {x['name'] for x in parameters}:
                                raise ValueError(f'LuaSurface.{token}: unknown parameter {name}')
                            self.used.add(('runtime', 'LuaSurface.' + token, name))
        # The vehicle property loop uses p[key]; extract its literal key list.
        if path.name == 'prototype-export.lua':
            for match in re.finditer(r'for _, key in ipairs\((\{.*?\})\)', path.read_text(), re.S):
                for key in re.findall(r'"([^"]+)"', match[1]):
                    self.check('runtime', 'LuaEntityPrototype', key)
        if path.name == 'recorder.lua':
            text = path.read_text()
            acceleration = re.findall(r'acceleration = (.*)', text)
            direction = re.findall(r'local direction = (.*)', text)
            for enum, lines in [('acceleration', acceleration), ('direction', direction)]:
                for name in re.findall(r'"([^"]+)"', '\n'.join(lines)):
                    self.define(['riding', enum, name])
        # The recorder passes this Tile array to LuaSurface.set_tiles.
        if path.name == 'recorder.lua':
            for i in range(len(ts) - 6):
                if ts[i:i + 6] == ['tiles', '[', '#', 'tiles', '+', '1']:
                    start = ts.index('{', i + 6)
                    self.record(ts, start, 'Tile')
        return ts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('contents', type=Path, help='installed Factorio Contents directory')
    parser.add_argument('--mod', type=Path, default=ROOT / 'mod/fsim-export')
    parser.add_argument('--list', action='store_true', help='print each checked API name')
    args = parser.parse_args()
    try:
        docs = args.contents.expanduser() / 'doc-html'
        runtime = json.loads((docs / 'runtime-api.json').read_text())
        prototype = json.loads((docs / 'prototype-api.json').read_text())
        if runtime['application_version'] != prototype['application_version']:
            raise ValueError('runtime/prototype API versions differ')
        audit = Audit(runtime, prototype)
        paths = sorted(args.mod.glob('*.lua'))
        if args.mod.name == 'fsim-export':
            paths += sorted((args.mod.parent / 'fsim-autorun').glob('*.lua'))
            paths += sorted((args.mod.parent / 'fsim-autodrive').glob('*.lua'))
        sources = [audit.file(path) for path in paths]
        # All prototype type strings in filters, the kind map and data:extend.
        types = {p['typename'] for p in prototype['prototypes'] if 'typename' in p}
        text = '\n'.join(' '.join(ts) for ts in sources)
        used_types = set()
        for match in re.finditer(r'\btype = (\{[^}]*\}|"[^"]+")', text):
            used_types.update(re.findall(r'"([^"]+)"', match[1]))
        normalize = args.mod / 'normalize.lua'
        if normalize.exists():
            kind = normalize.read_text().split('function M.kind(t)')[1].split('\nend')[0]
            used_types.update(re.findall(r'(\w+)\s*=\s*"', kind))
            used_types.update(re.findall(r'\["([^"\]]+)"\]', kind))
        final = args.mod / 'data-final-fixes.lua'
        if final.exists():
            used_types.update(re.findall(r'data\.raw\.(\w+)', final.read_text()))
        for name in used_types:
            if name not in types and name != 'bool-setting':
                raise ValueError(f'unknown prototype type {name}')
        if args.list:
            for stage, owner, name in sorted(audit.used):
                print(f'{stage}: {owner}.{name}')
        print(f'Factorio {runtime["application_version"]}: checked {len(sources)} Lua files, '
              f'{len(audit.used)} API names/record fields, {len(used_types)} prototype types')
        print('MapGenSettings, autoplace dictionaries, CliffPlacementSettings, method tables and riding enums: OK')
        print('Boolean settings checked against local schema (not present in API JSON)')
        print('API name audit passed')
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f'check-mod-api: {error}\n')


if __name__ == '__main__':
    main()
