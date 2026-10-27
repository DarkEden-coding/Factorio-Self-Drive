//! Index-backed real map pool. Files are loaded on demand, not retained in RAM.
use crate::{map_format, map_import::MapLoadReport, world::World, Prototypes};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Training uses `train` maps; evaluation defaults to `holdout`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    Train,
    Holdout,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapEntry {
    pub name: String,
    pub file: String,
    pub seed: u32,
    pub width: u32,
    pub height: u32,
    pub x_min: i32,
    pub y_min: i32,
    pub source_offset: [i32; 2],
    pub entity_count: usize,
    pub split: Split,
}

pub struct MapPool {
    directory: PathBuf,
    pub entries: Vec<MapEntry>,
    rng: Pcg32,
}
impl MapPool {
    /// Accept either a directory containing index.json or the index file itself.
    pub fn load(path: impl AsRef<Path>, seed: u64) -> Result<Self, String> {
        let path = path.as_ref();
        let index = if path.is_dir() {
            path.join("index.json")
        } else {
            path.to_path_buf()
        };
        let entries: Vec<MapEntry> =
            serde_json::from_slice(&std::fs::read(&index).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if entries.is_empty() {
            return Err("map pool is empty".into());
        }
        for entry in &entries {
            if !Path::new(&entry.file)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            {
                return Err("map file must be relative to the index directory".into());
            }
        }
        Ok(Self {
            directory: index.parent().unwrap_or(Path::new(".")).to_path_buf(),
            entries,
            rng: Pcg32::seed_from_u64(seed),
        })
    }
    pub fn map(&self, index: usize, data: &Prototypes) -> Result<(World, MapLoadReport), String> {
        let entry = self
            .entries
            .get(index)
            .ok_or("map pool index out of range")?;
        let bytes = std::fs::read(self.directory.join(&entry.file)).map_err(|e| e.to_string())?;
        let (world, report) = map_format::load_map(&bytes, data)?;
        let i = &world.info;
        if i.name != entry.name
            || i.seed != entry.seed
            || i.width != entry.width
            || i.height != entry.height
            || i.x_min != entry.x_min
            || i.y_min != entry.y_min
            || i.source_offset != Some(entry.source_offset)
            || world.entities.len() != entry.entity_count
        {
            return Err("map pool index does not match map metadata".into());
        }
        Ok((world, report))
    }
    /// Pool indices of the entries in `split`, in index order.
    pub fn split_indices(&self, split: Split) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|i| self.entries[*i].split == split)
            .collect()
    }
    pub fn sample(&mut self, data: &Prototypes) -> Result<(World, MapLoadReport), String> {
        let index = self.rng.gen_range(0..self.entries.len());
        self.map(index, data)
    }
}
