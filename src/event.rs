use crate::packet;
use crate::packet::{Deserialize, Serialize};
use bevy::prelude::{Entity, Event};
use std::cmp::Ordering;
use std::marker::PhantomData;

#[derive(Event)]
pub struct ChatMessageEvent {
    pub from: String,
    pub message: String,
}

#[derive(Event)]
pub struct SystemMessageEvent {
    pub message: String,
}

#[derive(Event)]
pub enum PlayerPositionAndLookEvent {
    PositionAndLook {
        entity_id: u32,
        x: f64,
        y: f64,
        z: f64,
        stance: f64,
        yaw: f32,
        pitch: f32,
    },
    Position {
        entity_id: u32,
        x: f64,
        y: f64,
        z: f64,
        stance: f64,
    },
    Look {
        entity_id: u32,
        yaw: f32,
        pitch: f32,
    },
}

#[derive(Copy, Clone, Debug)]
pub enum Face {
    Bottom = 0,
    Top = 1,
    Back = 2,
    Front = 3,
    Left = 4,
    Right = 5,
    UNKNOWN,
}

impl From<u8> for Face {
    fn from(value: u8) -> Self {
        match value {
            0 => Face::Bottom,
            1 => Face::Top,
            2 => Face::Back,
            3 => Face::Front,
            4 => Face::Left,
            5 => Face::Right,
            _ => Face::UNKNOWN,
        }
    }
}
#[derive(Event, Debug)]
pub enum PlayerDiggingEvent {
    Started {
        entity: Entity,
        x: i32,
        y: i8,
        z: i32,
        face: Face,
    },
    InProgress {
        entity: Entity,
    },
    Stopped {
        entity: Entity,
    },
    Completed {
        entity: Entity,
    },
}

#[derive(Event)]
pub struct BlockChangeEvent {
    pub x: i32,
    pub y: i8,
    pub z: i32,
    pub ty: u8,
    pub metadata: u8,
}

#[derive(Event, PartialEq, Eq)]
pub struct SendPacketEvent {
    pub entity: Entity,
    pub ord: usize,
    pub bytes: Vec<u8>,
}

impl SendPacketEvent {
    pub fn new<T: Serialize>(entity: Entity, packet: T) -> Result<Self, packet::PacketError> {
        Ok(Self {
            entity,
            ord: 5,
            bytes: packet.serialize()?,
        })
    }

    pub fn with_ord<T: Serialize>(
        entity: Entity,
        ord: usize,
        packet: T,
    ) -> Result<Self, packet::PacketError> {
        Ok(Self {
            entity,
            ord,
            bytes: packet.serialize()?,
        })
    }
}

impl PartialOrd for SendPacketEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SendPacketEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ord.cmp(&other.ord)
    }
}

#[derive(Event)]
pub struct AnimationEvent {
    pub entity: Entity,
    pub animation: u8,
}
