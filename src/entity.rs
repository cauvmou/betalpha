use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, RwLock};
use bevy_ecs::prelude::{Bundle, Component};
use tokio::net::TcpStream;

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
    pub struct Handshake;
    pub struct Login;
    pub struct Playing;
    impl ConnectionState for Handshake {}
    impl ConnectionState for Login {}
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

#[derive(Bundle)]
pub struct PlayerBundle<S: ConnectionState + Sized + Send + Sync + 'static> {
    pub stream: ClientStream<S>,
    pub position: Position,
    pub velocity: Velocity,
    pub look: Look,
    pub name: Named,
}