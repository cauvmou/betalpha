use crate::packet::{Deserialize, Serialize};
use bevy::prelude::{Entity, Event};
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
