use crate::entity::connection_state;
use crate::entity::{
    ClientStream, Look, Named, PlayerBundle, PlayerChunkDB, PlayerEntityDB, Position, Velocity,
};
use crate::event::PlayerPositionAndLookEvent;
use crate::packet::{ids, to_client_packets, to_server_packets, PacketError};
use crate::packet::{Deserialize, Serialize};
use crate::world::{Chunk, World};
use crate::{event, packet, util, TcpWrapper, BUFFER_SIZE};
use bevy::prelude::{Commands, Entity, EventReader, EventWriter, Mut, Query, Res, ResMut, With};
use bytes::{Buf, BufMut, BytesMut};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::io::{BufReader, Cursor, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

pub fn keep_alive(mut query: Query<&ClientStream, With<connection_state::Playing>>) {
    for stream in &mut query {
        let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
        stream.write_all(&[0x00]).unwrap();
        stream.flush().unwrap();
    }
}

pub fn disconnecting(
    (mut query, mut other): (
        Query<(Entity, &ClientStream, &connection_state::Disconnecting)>,
        Query<(Entity, &ClientStream, &PlayerEntityDB), With<connection_state::Playing>>,
    ),
    mut commands: Commands,
) {
    for (entity, stream_component, state) in &mut query {
        let mut stream: RwLockWriteGuard<'_, TcpStream> = stream_component.stream.write().unwrap();
        let packet = to_client_packets::KickPacket {
            reason: state.reason.clone(),
        };
        stream.write_all(&packet.serialize().unwrap()).unwrap();
        stream.flush().unwrap();
        commands.entity(entity).despawn();
        // Delete player for other players
        for (other, stream_component, db) in &mut other {
            let mut stream: RwLockWriteGuard<'_, TcpStream> =
                stream_component.stream.write().unwrap();
            let mut list: RwLockWriteGuard<Vec<u32>> = db.visible_entities.write().unwrap();
            if let Some(index) = list.iter().position(|p| *p == entity.index()) {
                list.swap_remove(index);
            }
            let packet = to_client_packets::DestroyEntityPacket {
                entity_id: entity.index(),
            };
            stream.write_all(&packet.serialize().unwrap()).unwrap();
            stream.flush().unwrap();
        }
    }
}

pub fn chat_message(
    mut chat_message_event_collector: EventReader<event::ChatMessageEvent>,
    mut query: Query<&ClientStream, With<connection_state::Playing>>,
) {
    let messages = chat_message_event_collector.read().collect::<Vec<_>>();
    for stream in &mut query {
        {
            let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
            messages.iter().for_each(|m| {
                let packet = to_client_packets::ChatMessagePacket {
                    message: format!("<{}> {}", m.from, m.message),
                };
                stream.write_all(&packet.serialize().unwrap()).unwrap();
            });
            stream.flush().unwrap();
        }
    }
}

pub fn calculate_visible_players(
    (mut query_entities, mut other): (
        Query<
            (Entity, &ClientStream, &mut PlayerEntityDB, &PlayerChunkDB),
            With<connection_state::Playing>,
        >,
        Query<(Entity, &Position, &Look, &Named), With<connection_state::Playing>>,
    ),
    mut commands: Commands,
) {
    for (entity, stream_component, player_db, chunk_db) in &mut query_entities {
        let mut stream: RwLockWriteGuard<'_, TcpStream> = stream_component.stream.write().unwrap();
        let mut list: RwLockWriteGuard<Vec<u32>> = player_db.visible_entities.write().unwrap();
        let chunks: &HashMap<(i32, i32), Arc<RwLock<Chunk>>> = &chunk_db.chunks;
        for (other, other_position, other_look, other_name_component) in &mut other {
            let other: Entity = other;
            let Position { x, z, .. } = &other_position;
            if entity.index() == other.index() {
                continue;
            }
            let is_inside_visible_chunks = chunks
                .values()
                .map(|c| {
                    let c = c.read().unwrap();
                    if c.is_inside_chunk(x.round() as i32, z.round() as i32) {
                        1
                    } else {
                        0
                    }
                })
                .sum::<usize>()
                > 0;
            match (is_inside_visible_chunks, list.contains(&other.index())) {
                (true, false) => {
                    list.push(other.index());
                    let packet = to_client_packets::EntityPacket {
                        entity_id: other.index(),
                    };
                    stream.write_all(&packet.serialize().unwrap()).unwrap();
                    stream.flush().unwrap();
                    let (rotation, pitch) = util::pack_float_pair(other_look.yaw, other_look.pitch);
                    let packet = to_client_packets::NamedEntitySpawnPacket {
                        entity_id: other.index(),
                        name: other_name_component.name.clone(),
                        x: (other_position.x * 32.0).round() as i32,
                        y: (other_position.y * 32.0).round() as i32,
                        z: (other_position.z * 32.0).round() as i32,
                        rotation,
                        pitch,
                        current_item: 0,
                    };
                    stream.write_all(&packet.serialize().unwrap()).unwrap();
                    stream.flush().unwrap();
                    debug!(
                        "Sent spawn entity: {} to entity: {}",
                        other.index(),
                        entity.index()
                    );
                }
                (false, true) => {
                    let index = list.iter().position(|p| *p == other.index()).unwrap();
                    list.swap_remove(index);
                    let packet = to_client_packets::DestroyEntityPacket {
                        entity_id: other.index(),
                    };
                    stream.write_all(&packet.serialize().unwrap()).unwrap();
                    stream.flush().unwrap();
                    debug!(
                        "Sent delete entity: {} to entity: {}",
                        other.index(),
                        entity.index()
                    );
                }
                (_, _) => {}
            }
        }
    }
}

pub fn player_movement(
    mut event_collector: EventReader<event::PlayerPositionAndLookEvent>,
    mut query: Query<(Entity, &mut Position, &mut Look), With<connection_state::Playing>>,
) {
    let events = event_collector.read().collect::<Vec<_>>();
    for (entity, mut position, mut look) in &mut query {
        for event in events.clone() {
            match event {
                PlayerPositionAndLookEvent::PositionAndLook {
                    entity_id,
                    x,
                    y,
                    z,
                    stance,
                    yaw,
                    pitch,
                } => {
                    if entity.index() == *entity_id {
                        position.x = *x;
                        position.y = *y;
                        position.z = *z;
                        position.stance = *stance;
                        look.yaw = *yaw;
                        look.pitch = *pitch;
                    }
                }
                PlayerPositionAndLookEvent::Position {
                    entity_id,
                    x,
                    y,
                    z,
                    stance,
                } => {
                    if entity.index() == *entity_id {
                        position.x = *x;
                        position.y = *y;
                        position.z = *z;
                        position.stance = *stance;
                    }
                }
                PlayerPositionAndLookEvent::Look {
                    entity_id,
                    yaw,
                    pitch,
                } => {
                    if entity.index() == *entity_id {
                        look.yaw = *yaw;
                        look.pitch = *pitch;
                    }
                }
            }
        }
    }
}

pub fn load_chunks(
    mut world: ResMut<World>,
    mut query: Query<
        (Entity, &Position, &ClientStream, &mut PlayerChunkDB),
        With<connection_state::Playing>,
    >,
) {
    for (entity, position, stream_component, mut db) in &mut query {
        // Get players chunk
        let x = position.x.round() as i32;
        let z = position.z.round() as i32;
        let (player_chunk_x, player_chunk_z) = ((x - x % 16) / 16, (z - z % 16) / 16);

        let mut stream: RwLockWriteGuard<'_, TcpStream> = stream_component.stream.write().unwrap();

        let chunk_r = crate::INITIAL_CHUNK_LOAD_SIZE / 2;
        for x in (player_chunk_x - chunk_r)..(player_chunk_x + chunk_r) {
            for z in (player_chunk_z - chunk_r)..(player_chunk_z + chunk_r) {
                match world.get_chunk(x, z) {
                    Ok(chunk) => {
                        //debug!("Loaded chunk at (x: {x}, z: {z}).");
                        if db.chunks.insert((x, z), chunk.clone()).is_none() {
                            stream
                                .write_all(
                                    &to_client_packets::PreChunkPacket { x, z, mode: true }
                                        .serialize()
                                        .unwrap(),
                                )
                                .unwrap();

                            let (len, chunk_data) = chunk.read().unwrap().get_compressed_data();

                            stream
                                .write_all(
                                    &to_client_packets::MapChunkPacket {
                                        x: x * 16,
                                        y: 0,
                                        z: z * 16,
                                        size_x: 15,
                                        size_y: 127,
                                        size_z: 15,
                                        compressed_size: len,
                                        compressed_data: chunk_data[..len as usize].to_vec(),
                                    }
                                    .serialize()
                                    .unwrap(),
                                )
                                .unwrap();
                        }
                    }
                    Err(err) => {
                        //error!("Failed to load chunk at (x: {x}, z: {z}): {err}!")
                    }
                }
            }
        }
        stream.flush().unwrap();
    }
}

pub fn unload_chunks(
    mut world: ResMut<World>,
    mut query: Query<
        (Entity, &Position, &ClientStream, &mut PlayerChunkDB),
        With<connection_state::Playing>,
    >,
) {
    for (entity, position, stream_component, mut db) in &mut query {
        // Get players chunk
        let x = position.x.round() as i32;
        let z = position.z.round() as i32;
        let (player_chunk_x, player_chunk_z) = ((x - x % 16) / 16, (z - z % 16) / 16);

        let mut stream: RwLockWriteGuard<'_, TcpStream> = stream_component.stream.write().unwrap();

        let chunk_r = crate::INITIAL_CHUNK_LOAD_SIZE / 2;
        let mut loaded = Vec::with_capacity(
            crate::INITIAL_CHUNK_LOAD_SIZE as usize * crate::INITIAL_CHUNK_LOAD_SIZE as usize,
        );
        for x in (player_chunk_x - chunk_r)..(player_chunk_x + chunk_r) {
            for z in (player_chunk_z - chunk_r)..(player_chunk_z + chunk_r) {
                loaded.push((x, z));
            }
        }
        let to_remove = db
            .chunks
            .keys()
            .filter(|k| !loaded.contains(*k))
            .copied()
            .collect::<Vec<_>>();
        for (x, z) in to_remove {
            db.chunks.remove(&(x, z));
            stream
                .write_all(
                    &to_client_packets::PreChunkPacket { x, z, mode: false }
                        .serialize()
                        .unwrap(),
                )
                .unwrap();
        }
    }
}
