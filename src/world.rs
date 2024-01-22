use crate::util::{base36_from_i32, base36_from_u64};
use crate::world::util::{
    read_nbt_bool, read_nbt_byte_array, read_nbt_i32, read_nbt_i64, read_value_bool,
    read_value_byte_array, read_value_i32, read_value_i64,
};
use bevy::prelude::Resource;
use log::debug;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, RwLock, TryLockResult};

mod util {
    pub fn read_nbt_i64(blob: &nbt::Blob, name: &'static str) -> std::io::Result<i64> {
        if let nbt::Value::Long(v) = blob.get(name).ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Field does not exist!",
        ))? {
            return Ok(*v);
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }

    pub fn read_nbt_i32(blob: &nbt::Blob, name: &'static str) -> std::io::Result<i32> {
        if let nbt::Value::Int(v) = blob.get(name).ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Field does not exist!",
        ))? {
            return Ok(*v);
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }

    pub fn read_nbt_byte(blob: &nbt::Blob, name: &'static str) -> std::io::Result<i8> {
        if let nbt::Value::Byte(v) = blob.get(name).ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Field does not exist!",
        ))? {
            return Ok(*v);
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }

    pub fn read_nbt_bool(blob: &nbt::Blob, name: &'static str) -> std::io::Result<bool> {
        read_nbt_byte(blob, name).map(|v| v > 0)
    }

    pub fn read_nbt_byte_array(blob: &nbt::Blob, name: &'static str) -> std::io::Result<Vec<u8>> {
        if let nbt::Value::ByteArray(v) = blob.get(name).ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Field does not exist!",
        ))? {
            return Ok(unsafe {
                let slice = std::ptr::slice_from_raw_parts(v.as_ptr() as *const u8, v.len());
                Vec::from(slice.as_ref().unwrap())
            });
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }

    pub fn read_value_i64(value: &nbt::Value) -> std::io::Result<i64> {
        if let nbt::Value::Long(v) = value {
            return Ok(*v);
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }

    pub fn read_value_i32(value: &nbt::Value) -> std::io::Result<i32> {
        if let nbt::Value::Int(v) = value {
            return Ok(*v);
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }

    pub fn read_value_byte(value: &nbt::Value) -> std::io::Result<i8> {
        if let nbt::Value::Byte(v) = value {
            return Ok(*v);
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }

    pub fn read_value_bool(value: &nbt::Value) -> std::io::Result<bool> {
        read_value_byte(value).map(|v| v > 0)
    }

    pub fn read_value_byte_array(value: &nbt::Value) -> std::io::Result<Vec<u8>> {
        if let nbt::Value::ByteArray(v) = value {
            return Ok(unsafe {
                let slice = std::ptr::slice_from_raw_parts(v.as_ptr() as *const u8, v.len());
                Vec::from(slice.as_ref().unwrap())
            });
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Field has wrong type!",
            ))
        }
    }
}

#[derive(Resource)]
pub struct World {
    path: PathBuf,
    chunks: HashMap<(i32, i32), Arc<RwLock<Chunk>>>,
    seed: i64,
    spawn: [i32; 3],
    time: u64,
    size_on_disk: u64,
    last_played: u64,
}

impl World {
    pub fn open<P: AsRef<Path>>(world_path: P) -> std::io::Result<Self> {
        let (seed, spawn, time, size_on_disk, last_played) = {
            let mut file = std::fs::File::open(world_path.as_ref().join("level.dat"))?;
            let blob = nbt::Blob::from_gzip_reader(&mut file)?;

            let data = blob.get("Data").unwrap();

            if let nbt::Value::Compound(v) = data {
                let seed = read_value_i64(v.get("RandomSeed").unwrap())?;
                let spawn = [
                    read_value_i32(v.get("SpawnX").unwrap())?,
                    read_value_i32(v.get("SpawnY").unwrap())?,
                    read_value_i32(v.get("SpawnZ").unwrap())?,
                ];
                let time = read_value_i64(v.get("Time").unwrap())? as u64;
                let size_on_disk = read_value_i64(v.get("SizeOnDisk").unwrap())? as u64;
                let last_player = read_value_i64(v.get("LastPlayed").unwrap())? as u64;
                (seed, spawn, time, size_on_disk, last_player)
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Did not find Level field in chunk!",
                ));
            }
        };

        Ok(Self {
            path: world_path.as_ref().to_path_buf(),
            chunks: HashMap::with_capacity(u16::MAX as usize),
            seed,
            spawn,
            time,
            size_on_disk,
            last_played,
        })
    }

    pub fn close(self) -> std::io::Result<()> {
        let mut file = std::fs::File::create(self.path.join("level.dat"))?;
        let mut compund = HashMap::with_capacity(7);
        compund.insert("RandomSeed".to_string(), nbt::Value::Long(self.seed as i64));
        compund.insert("SpawnX".to_string(), nbt::Value::Int(self.spawn[0]));
        compund.insert("SpawnY".to_string(), nbt::Value::Int(self.spawn[1]));
        compund.insert("SpawnZ".to_string(), nbt::Value::Int(self.spawn[2]));
        compund.insert("Time".to_string(), nbt::Value::Long(self.time as i64));
        let size = fs_extra::dir::get_size(self.path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        compund.insert("SizeOnDisk".to_string(), nbt::Value::Long(size as i64));
        compund.insert(
            "LastPlayed".to_string(),
            nbt::Value::Long(std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() as i64),
        );

        let mut blob = nbt::Blob::new();
        blob.insert("Data", nbt::Value::Compound(compund))?;
        blob.to_gzip_writer(&mut file)?;
        Ok(())
    }

    /// Gets a chunk from loaded chunks or loads the chunk into memory.
    ///
    /// returns: Result<Rc<RefCell<Chunk>, Global>, Error>
    pub fn get_chunk(&mut self, x: i32, z: i32) -> std::io::Result<Arc<RwLock<Chunk>>> {
        let key = (x, z);
        if let Some(chunk) = self.chunks.get(&key) {
            Ok(chunk.clone())
        } else {
            let chunk = Chunk::load(&self.path, x, z)?;
            self.chunks.insert(key, Arc::new(RwLock::new(chunk)));
            self.chunks.get(&key).cloned().ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Chunk is not loaded!",
            ))
        }
    }

    /// Saves a chunk to disk and unloads it from memory.
    ///
    /// Errors if chunk is still borrowed.
    ///
    /// returns: Result<(), Error>
    pub fn unload_chunk(&mut self, x: i32, z: i32) -> std::io::Result<()> {
        let key = (x, z);

        if let Some(chunk) = self.chunks.remove(&key) {
            match chunk.try_write() {
                Ok(mut chunk) => chunk.save(&self.path),
                Err(e) => {
                    self.chunks.insert(key, chunk.clone());
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                }
            }
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Chunk is not loaded!",
            ))
        }
    }

    pub fn save_chunk(&mut self, x: i32, z: i32) -> std::io::Result<()> {
        let chunk = self.get_chunk(x, z)?;
        let chunk = chunk.try_write();
        match chunk {
            Ok(chunk) => chunk.save(&self.path),
            Err(err) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )),
        }
    }

    pub fn get_seed(&self) -> i64 {
        self.seed
    }

    #[inline]
    pub fn get_spawn(&self) -> [i32; 3] {
        self.spawn
    }

    pub fn get_time(&self) -> u64 {
        self.time
    }

    pub fn set_time(&mut self, time: u64) {
        self.time = time;
        if time >= 24000 {
            self.time -= 24000;
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
    pub fn load(world_path: &Path, x: i32, z: i32) -> std::io::Result<Self> {
        let (x_string, z_string) = (base36_from_i32(x), base36_from_i32(z));
        let (high_level, low_level) = (
            base36_from_u64((((x as i8) as u8) % 64) as u64),
            base36_from_u64((((z as i8) as u8) % 64) as u64),
        );
        let file_name = format!("c.{x_string}.{z_string}.dat");
        let file_path = world_path.join(high_level).join(low_level).join(file_name);

        let (terrain_populated, last_update, blocks, data, block_light, sky_light, height_map) = {
            let mut file = std::fs::File::open(file_path)?;
            let blob = nbt::Blob::from_gzip_reader(&mut file)?;
            let data = blob.get("Level").unwrap();

            if let nbt::Value::Compound(v) = data {
                let terrain_populated = read_value_bool(v.get("TerrainPopulated").unwrap())?;
                let last_update = read_value_i64(v.get("LastUpdate").unwrap())? as u64;
                let blocks = read_value_byte_array(v.get("Blocks").unwrap())?;
                let data = read_value_byte_array(v.get("Data").unwrap())?;
                let block_light = read_value_byte_array(v.get("BlockLight").unwrap())?;
                let sky_light = read_value_byte_array(v.get("SkyLight").unwrap())?;
                let height_map = read_value_byte_array(v.get("HeightMap").unwrap())?;
                (
                    terrain_populated,
                    last_update,
                    blocks,
                    data,
                    block_light,
                    sky_light,
                    height_map,
                )
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Did not find Level field in chunk!",
                ));
            }
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

    /// Returns the BlockID at the coordinates specified or `None` if the index is out of bounds.
    ///
    /// # Arguments
    ///
    /// * `x`: chunk local x
    /// * `y`: chunk local y
    /// * `z`: chunk local z
    ///
    /// returns: Option<u8>
    pub fn get_block(&self, x: u8, y: u8, z: u8) -> Option<u8> {
        let index = (y as i32 + ((z as i32) * 128 + ((x as i32) * 128 * 16))) as usize;
        self.blocks.get(index).copied()
    }

    /// Overwrites the BlockID at the coordinates specified and returns the old BlockID or `None` if the index is out of bounds.
    ///
    /// # Arguments
    ///
    /// * `x`: chunk local x
    /// * `y`: chunk local y
    /// * `z`: chunk local z
    ///
    /// returns: Option<u8>
    pub fn set_block(&mut self, x: u8, y: u8, z: u8, block_id: u8) -> Option<u8> {
        let index = (y as i32 + ((z as i32) * 128 + ((x as i32) * 128 * 16))) as usize;
        self.blocks.get_mut(index).map(|v| {
            let tmp = *v;
            *v = block_id;
            tmp
        })
    }

    pub fn save(&self, world_path: &Path) -> std::io::Result<()> {
        let (x_string, z_string) = (self.chunk_x, self.chunk_z);
        let (high_level, low_level) = (
            base36_from_u64(self.chunk_x as u64 % 64),
            base36_from_u64(self.chunk_z as u64 % 64),
        );
        let file_name = format!("c.{x_string}.{z_string}.dat");
        let file_path = world_path.join(high_level).join(low_level).join(file_name);

        {
            let vu8_vi8 = |x: &Vec<u8>| -> Vec<i8> {
                unsafe {
                    let slice = std::ptr::slice_from_raw_parts(x.as_ptr() as *const i8, x.len());
                    Vec::from(slice.as_ref().unwrap())
                }
            };

            let mut file = std::fs::File::create(file_path)?;
            let mut compound = HashMap::with_capacity(7);
            compound.insert(
                "TerrainPopulated".to_string(),
                nbt::Value::Byte(self.terrain_populated as i8),
            );
            compound.insert(
                "LastUpdate".to_string(),
                nbt::Value::Long(self.last_update as i64),
            );
            compound.insert(
                "Blocks".to_string(),
                nbt::Value::ByteArray(vu8_vi8(&self.blocks)),
            );
            compound.insert(
                "Data".to_string(),
                nbt::Value::ByteArray(vu8_vi8(&self.data)),
            );
            compound.insert(
                "BlockLight".to_string(),
                nbt::Value::ByteArray(vu8_vi8(&self.block_light)),
            );
            compound.insert(
                "SkyLight".to_string(),
                nbt::Value::ByteArray(vu8_vi8(&self.sky_light)),
            );
            compound.insert(
                "HeightMap".to_string(),
                nbt::Value::ByteArray(vu8_vi8(&self.height_map)),
            );

            let mut blob = nbt::Blob::new();
            blob.insert("Level", nbt::Value::Compound(compound))?;
            blob.to_gzip_writer(&mut file)?;
        }

        Ok(())
    }

    pub fn is_inside_chunk(&self, x: i32, z: i32) -> bool {
        let (chunk_x, chunk_z) = ((x - x % 16) / 16, (z - z % 16) / 16);
        self.chunk_x == chunk_x && self.chunk_z == chunk_z
    }

    pub fn get_compressed_data(&self) -> (i32, Vec<u8>) {
        let mut to_compress = self.blocks.clone();
        to_compress.extend_from_slice(&self.data);
        to_compress.extend_from_slice(&self.block_light);
        to_compress.extend_from_slice(&self.sky_light);
        let mut len = unsafe { libz_sys::compressBound(to_compress.len().try_into().unwrap()) };
        let mut compressed_bytes = vec![0u8; len as usize];
        unsafe {
            libz_sys::compress(
                compressed_bytes.as_mut_ptr(),
                &mut len,
                to_compress.as_ptr(),
                to_compress.len().try_into().unwrap(),
            );
        }
        (len as i32, compressed_bytes)
    }
}
