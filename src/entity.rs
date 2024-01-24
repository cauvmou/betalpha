use crate::packet::to_client_packets::PlayerInventoryPacket;
use crate::packet::PacketError;
use crate::world::Chunk;
use crate::{packet, BUFFER_SIZE};
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
    #[derive(Component)]
    pub struct Invalid;
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

#[derive(Component)]
pub struct Digging {
    pub x: i32,
    pub y: i8,
    pub z: i32,
    pub face: crate::event::Face,
}

#[derive(Copy, Clone)]
pub struct Item {
    pub id: u16,
    pub count: u8,
    pub uses_left: u16,
}

pub struct InventoryArea<const N: usize> {
    items: [Option<Item>; N],
}

impl<const N: usize> InventoryArea<N> {
    pub fn new() -> Self {
        Self { items: [None; N] }
    }

    pub fn create_with_data() -> Self {
        let mut s = Self::new();
        s.items[0] = Some(Item {
            id: 1,
            count: 64,
            uses_left: 0,
        });
        s
    }
}

#[derive(Component)]
pub struct Inventory {
    main: InventoryArea<36>,
    armor: InventoryArea<4>,
    crafting: InventoryArea<4>,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            main: InventoryArea::create_with_data(),
            armor: InventoryArea::new(),
            crafting: InventoryArea::new(),
        }
    }

    pub fn update_from_raw(&mut self, packet: PlayerInventoryPacket) {
        if let Some(inv) = match packet.inventory_type {
            -1 => Some(self.main.items.as_mut_slice()),
            -2 => Some(self.armor.items.as_mut_slice()),
            -3 => Some(self.crafting.items.as_mut_slice()),
            _ => None,
        } {
            for index in 0..packet.count as usize {
                inv[index] = packet.items[index].map(|v| Item {
                    id: v.item_id as u16,
                    count: v.count as u8,
                    uses_left: v.uses as u16,
                });
            }
        }
    }

    pub fn to_raw_packet(&self, inventory_type: i32) -> Result<PlayerInventoryPacket, PacketError> {
        if let Some(inv) = match inventory_type {
            -1 => Some(self.main.items.iter()),
            -2 => Some(self.armor.items.iter()),
            -3 => Some(self.crafting.items.iter()),
            _ => None,
        } {
            let packet = PlayerInventoryPacket {
                inventory_type,
                count: inv.len() as i16,
                items: inv
                    .map(|item| {
                        if let Some(item) = item {
                            Some(packet::to_client_packets::Item {
                                item_id: item.id as i16,
                                count: item.count as i8,
                                uses: item.uses_left as i16,
                            })
                        } else {
                            None
                        }
                    })
                    .collect(),
            };
            Ok(packet)
        } else {
            Err(PacketError::InvalidInput(format!(
                "Invalid inventory type: {inventory_type}"
            )))
        }
    }

    /*

    */
}
