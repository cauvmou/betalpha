use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::net::TcpStream;
use std::sync::{Arc, Mutex, RwLock};
use bevy::prelude::{Bundle, Component};
use crate::world::Chunk;

#[derive(Component, Default)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
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
    pub name: String
}

pub trait ConnectionState: Sized {}
pub mod connection_state {
    use crate::entity::ConnectionState;
    pub struct Login;
    pub struct Initializing;
    pub struct Playing;
    impl ConnectionState for Login {}
    impl ConnectionState for Initializing {}
    impl ConnectionState for Playing {}
}

#[derive(Component)]
pub struct ClientStream<S: ConnectionState> {
    pub stream: Arc<RwLock<TcpStream>>,
    _ty: PhantomData<S>
}

impl<S: ConnectionState> ClientStream<S> {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream: Arc::new(RwLock::new(stream)),
            _ty: Default::default(),
        }
    }

    pub fn from(stream: Arc<RwLock<TcpStream>>) -> Self {
        Self {
            stream,
            _ty: Default::default(),
        }
    }
}

#[derive(Component)]
pub struct PlayerChunkDB {
    pub chunks: HashMap<u64, Arc<RwLock<Chunk>>>,
}

#[derive(Bundle)]
pub struct PlayerBundle<S: ConnectionState + Sized + Send + Sync + 'static> {
    pub stream: ClientStream<S>,
    pub position: Position,
    pub velocity: Velocity,
    pub look: Look,
}