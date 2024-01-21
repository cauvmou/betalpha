use crate::world::Chunk;
use crate::BUFFER_SIZE;
use bevy::prelude::{Bundle, Component};
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::net::TcpStream;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Component, Default)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub stance: f64,
    pub on_ground: bool,
}

#[derive(Component, Default)]
pub struct PreviousPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub stance: f64,
    pub on_ground: bool,
}

impl PreviousPosition {
    pub fn distance_moved(&self, pos: &Position) -> f64 {
        let (x, y, z) = (pos.x - self.x, pos.y - self.y, pos.z - self.z);
        (x * x + y * y + z * z).sqrt()
    }

    pub fn relative_movement(&self, pos: &Position) -> (i8, i8, i8) {
        let x = ((pos.x - self.x) * 32.0).round() as i8;
        let y = ((pos.y - self.y) * 32.0).round() as i8;
        let z = ((pos.z - self.z) * 32.0).round() as i8;
        (x, y, z)
    }
}

#[derive(Component, Default)]
pub struct Velocity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Component, Default)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Component, Default)]
pub struct Named {
    pub name: String,
}

pub mod connection_state {
    use bevy::prelude::Component;

    #[derive(Component)]
    pub struct Login;
    #[derive(Component)]
    pub struct Initializing;
    #[derive(Component)]
    pub struct Playing;
    #[derive(Component)]
    pub struct Disconnecting {
        pub reason: String,
    }
}

#[derive(Component)]
pub struct ClientStream {
    pub stream: Arc<RwLock<TcpStream>>,
    pub left_over: Arc<RwLock<Vec<u8>>>,
}

impl ClientStream {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream: Arc::new(RwLock::new(stream)),
            left_over: Arc::new(RwLock::new(Vec::with_capacity(BUFFER_SIZE))),
        }
    }

    pub fn from(stream: Arc<RwLock<TcpStream>>) -> Self {
        Self {
            stream,
            left_over: Arc::new(RwLock::new(Vec::with_capacity(BUFFER_SIZE))),
        }
    }
}

#[derive(Component)]
pub struct PlayerChunkDB {
    pub chunks: HashMap<(i32, i32), Arc<RwLock<Chunk>>>,
}

#[derive(Component)]
pub struct PlayerEntityDB {
    pub visible_entities: Arc<RwLock<Vec<u32>>>,
}

#[derive(Bundle)]
pub struct PlayerBundle {
    pub stream: ClientStream,
    pub position: PreviousPosition,
    pub velocity: Velocity,
    pub look: Look,
}
