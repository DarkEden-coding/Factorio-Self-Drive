//! Compact real-map loading, shared by native and wasm callers.
use crate::{map_import, prototypes::Prototypes, world::World};
use flate2::read::ZlibDecoder;
use serde::Deserialize;
use std::{collections::BTreeSet, io::Read};

#[derive(Deserialize)]
struct Header {
    name: String,
    seed: u32,
    area: [[i32; 2]; 2],
    spawn: [f64; 2],
    tile_names: Vec<String>,
    resource_names: Vec<String>,
    entity_names: Vec<[String; 2]>,
    cliff_names: Vec<String>,
    entity_count: usize,
    #[serde(default)]
    warnings: Vec<String>,
    /// Source chunk positions of demolisher territory.
    #[serde(default)]
    zone_chunks: Vec<[i32; 2]>,
}

pub fn load_map(
    bytes: &[u8],
    data: &Prototypes,
) -> Result<(World, map_import::MapLoadReport), String> {
    if bytes.len() < 16 || &bytes[..8] != b"FSMAP001" {
        return Err("expected FSMAP001 map bytes".into());
    }
    let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let raw_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if header_len > 1_048_576 || header_len > bytes.len() - 16 {
        return Err("invalid map header length".into());
    }
    let header: Header =
        serde_json::from_slice(&bytes[16..16 + header_len]).map_err(|e| e.to_string())?;
    let [width, height] = map_import::dimensions(header.area)?;
    let area = width as usize * height as usize;
    if header.entity_count > 4_000_000 || raw_len != area * 2 + header.entity_count * 32 {
        return Err("invalid map body length".into());
    }
    if header.tile_names.is_empty()
        || header.tile_names.len() > 256
        || header.resource_names.first().map(String::as_str) != Some("none")
        || header.resource_names.len() > 256
        || header.entity_names.len() > 65536
        || header.cliff_names.len() > 65535
        || header
            .spawn
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 2_147_483_648.0)
    {
        return Err("invalid map name tables or spawn".into());
    }
    let mut body = Vec::new();
    let mut decoder = ZlibDecoder::new(&bytes[16 + header_len..]);
    (&mut decoder)
        .take(raw_len as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() != raw_len || decoder.total_in() as usize != bytes.len() - 16 - header_len {
        return Err("truncated, oversized, or trailing map body".into());
    }
    let offset = [
        header.area[0][0] + width as i32 / 2,
        header.area[0][1] + height as i32 / 2,
    ];
    let mut world = World::empty_rect(width, height, header.seed, 0);
    world.info.name = header.name.clone();
    world.info.source_offset = Some(offset);
    world.spawn = [
        header.spawn[0] - offset[0] as f64,
        header.spawn[1] - offset[1] as f64,
    ];
    let mut warnings: BTreeSet<String> = header.warnings.into_iter().collect();
    let tiles: Vec<_> = header
        .tile_names
        .iter()
        .map(|n| map_import::tile_id(n, data, &mut warnings))
        .collect();
    let resources: Vec<_> = header
        .resource_names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if i == 0 {
                0
            } else {
                map_import::resource_id(n, data, &mut warnings)
            }
        })
        .collect();
    let world_tiles = std::sync::Arc::make_mut(&mut world.tiles);
    for (index, id) in body[..area].iter().enumerate() {
        world_tiles[index] = *tiles.get(*id as usize).ok_or("invalid tile name index")?;
    }
    let world_resources = std::sync::Arc::make_mut(&mut world.resources);
    for (index, id) in body[area..area * 2].iter().enumerate() {
        world_resources[index] = *resources
            .get(*id as usize)
            .ok_or("invalid resource name index")?;
    }
    for record in body[area * 2..].as_chunks::<32>().0 {
        let name = u16::from_le_bytes(record[..2].try_into().unwrap()) as usize;
        let [name, kind] = header
            .entity_names
            .get(name)
            .ok_or("invalid entity name index")?;
        let x = f64::from_le_bytes(record[2..10].try_into().unwrap());
        let y = f64::from_le_bytes(record[10..18].try_into().unwrap());
        let orientation = f64::from_le_bytes(record[18..26].try_into().unwrap());
        let cliff = u16::from_le_bytes(record[26..28].try_into().unwrap());
        let health = f32::from_le_bytes(record[28..32].try_into().unwrap());
        if x < header.area[0][0] as f64
            || x >= header.area[1][0] as f64
            || y < header.area[0][1] as f64
            || y >= header.area[1][1] as f64
        {
            return Err("entity outside map area".into());
        }
        let entity = map_import::MapEntity {
            name: name.clone(),
            kind: kind.clone(),
            position: [x - offset[0] as f64, y - offset[1] as f64],
            orientation,
            health: (!health.is_nan()).then_some(health as f64),
            cliff_orientation: if cliff == u16::MAX {
                None
            } else {
                Some(
                    header
                        .cliff_names
                        .get(cliff as usize)
                        .ok_or("invalid cliff name index")?
                        .clone(),
                )
            },
        };
        map_import::validate_entity(&entity)?;
        map_import::add_entity(&mut world, data, &entity, &mut warnings)?;
    }
    world.set_territory_chunks(&header.zone_chunks);
    Ok((
        world,
        map_import::MapLoadReport {
            name: header.name,
            offset,
            warnings: warnings.into_iter().collect(),
        },
    ))
}
