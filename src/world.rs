use std::cell::{BorrowMutError, Ref, RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use rayon::prelude::*;
use crate::util::base36_from_u64;
use crate::world::util::{read_nbt_bool, read_nbt_byte_array, read_nbt_i32, read_nbt_i64};

mod util {
    pub fn read_nbt_i64(blob: &nbt::Blob, name: &'static str) -> std::io::Result<i64> {
        if let nbt::Value::Long(v) = blob.get(name).ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, "Field does not exist!"))? {
            return Ok(*v)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Field has wrong type!"))
        }
    }

    pub fn read_nbt_i32(blob: &nbt::Blob, name: &'static str) -> std::io::Result<i32> {
        if let nbt::Value::Int(v) = blob.get(name).ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, "Field does not exist!"))? {
            return Ok(*v)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Field has wrong type!"))
        }
    }

    pub fn read_nbt_byte(blob: &nbt::Blob, name: &'static str) -> std::io::Result<i8> {
        if let nbt::Value::Byte(v) = blob.get(name).ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, "Field does not exist!"))? {
            return Ok(*v)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Field has wrong type!"))
        }
    }

    pub fn read_nbt_bool(blob: &nbt::Blob, name: &'static str) -> std::io::Result<bool> {
        read_nbt_byte(blob, name).map(|v| v > 0)
    }

    pub fn read_nbt_byte_array(blob: &nbt::Blob, name: &'static str) -> std::io::Result<Vec<u8>> {
        if let nbt::Value::ByteArray(v) = blob.get(name).ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, "Field does not exist!"))? {
            return Ok(unsafe {
                let slice = std::ptr::slice_from_raw_parts(v.as_ptr() as *const u8, v.len());
                Vec::from(slice.as_ref().unwrap())
            })
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Field has wrong type!"))
        }
    }
}

pub struct World {
    path: PathBuf,
    chunks: HashMap<u64, Rc<RefCell<Chunk>>>,
    seed: u64,
    spawn: [i32; 3],
    time: u64,
    size_on_disk: u64,
    last_played: u64,
}

impl World {
    pub fn load(world_path: &PathBuf) -> std::io::Result<Self> {
        // parse level.dat
        let (seed, spawn, time, size_on_disk, last_played) = {
            let mut file = std::fs::File::open(world_path.join("level.dat"))?;
            let blob = nbt::Blob::from_gzip_reader(&mut file)?;

            let seed = read_nbt_i64(&blob, "RandomSeed")? as u64;
            let spawn = [read_nbt_i32(&blob, "SpawnX")?, read_nbt_i32(&blob, "SpawnY")?, read_nbt_i32(&blob, "SpawnZ")?];
            let time = read_nbt_i64(&blob, "Time")? as u64;
            let size_on_disk = read_nbt_i64(&blob, "SizeOnDisk")? as u64;
            let last_player = read_nbt_i64(&blob, "LastPlayed")? as u64;

            (seed, spawn, time, size_on_disk, last_player)
        };

        Ok(Self {
            path: world_path.clone(),
            chunks: HashMap::with_capacity(u16::MAX as usize),
            seed,
            spawn,
            time,
            size_on_disk,
            last_played,
        })
    }

    /// Gets a chunk from loaded chunks or loads the chunk into memory.
    ///
    /// returns: Result<Rc<RefCell<Chunk>, Global>, Error>
    pub fn get_chunk(&mut self, x: i32, z: i32) -> std::io::Result<Rc<RefCell<Chunk>>> {
        let key = (x as u64) << 4 | z as u64;
        if let Some(chunk) = self.chunks.get(&key) {
            Ok(chunk.clone())
        } else {
            let chunk = Chunk::load(&self.path, x, z)?;
            self.chunks.insert(key, Rc::new(RefCell::new(chunk)));
            self.chunks.get(&key).cloned().ok_or(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Chunk is not loaded!"))
        }
    }

    /// Saves a chunk to disk and unloads it from memory.
    ///
    /// Errors if chunk is still borrowed.
    ///
    /// returns: Result<(), Error>
    pub fn unload_chunk(&mut self, x: i32, z: i32) -> std::io::Result<()> {
        let key = (x as u64) << 4 | z as u64;

        if let Some(chunk) = self.chunks.remove(&key) {
            match chunk.try_borrow_mut() {
                Ok(mut chunk) => {chunk.save()}
                Err(e) => {
                    self.chunks.insert(key, chunk.clone());
                    Err(std::io::Error::new(std::io::ErrorKind::Other, e))
                }
            }
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Chunk is not loaded!"))
        }
    }
}

pub struct Chunk {
    chunk_x: i32,
    chunk_z: i32,
    terrain_populated: bool,
    last_update: u64,
    blocks: Vec<u8>,
    data: Vec<u8>,
    block_light: Vec<u8>,
    sky_light: Vec<u8>,
    height_map: Vec<u8>,
}

impl Chunk {
    pub fn load(world_path: &PathBuf, x: i32, z: i32) -> std::io::Result<Self> {
        let (x_string, z_string) = (base36_from_u64(x as u64), base36_from_u64(z as u64));
        let (high_level, low_level) = (base36_from_u64(x as u64 % 64), base36_from_u64(z as u64 % 64));
        let file_name = format!("c.{x_string}.{z_string}.dat");
        let file_path = world_path.join(high_level).join(low_level).join(file_name);

        let (terrain_populated, last_update, blocks, data, block_light, sky_light, height_map) = {
            let mut file = std::fs::File::open(file_path)?;
            let blob = nbt::Blob::from_gzip_reader(&mut file)?;

            let terrain_populated = read_nbt_bool(&blob, "TerrainPopulated")?;
            let last_update = read_nbt_i64(&blob, "LastUpdate")? as u64;
            let blocks = read_nbt_byte_array(&blob, "Blocks")?;
            let data = read_nbt_byte_array(&blob, "Data")?;
            let block_light = read_nbt_byte_array(&blob, "BlockLight")?;
            let sky_light = read_nbt_byte_array(&blob, "SkyLight")?;
            let height_map = read_nbt_byte_array(&blob, "HeightMap")?;
            (terrain_populated, last_update, blocks, data, block_light, sky_light, height_map)
        };

        Ok(Self {
            chunk_x: x,
            chunk_z: z,
            terrain_populated,
            last_update,
            blocks,
            data,
            block_light,
            sky_light,
            height_map,
        })
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        todo!()
    }
}

