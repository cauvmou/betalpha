use crate::entity::{connection_state, Digging, PreviousPosition};
use crate::entity::{Look, Named, PlayerBundle, PlayerChunkDB, PlayerEntityDB, Position, Velocity};
use crate::event::{
    AnimationEvent, BlockChangeEvent, PlayerDiggingEvent, PlayerPositionAndLookEvent,
    PlayerUseEvent, SendPacketEvent,
};
use crate::packet::{ids, to_client_packets, to_server_packets, PacketError};
use crate::packet::{Deserialize, Serialize};
use crate::world::{Chunk, World};
use crate::{event, packet, util, TcpWrapper, BUFFER_SIZE};
use bevy::prelude::{Commands, Entity, EventReader, EventWriter, Mut, Query, Res, ResMut, With};
use bevy::utils::tracing::Instrument;
use bytes::{Buf, BufMut, BytesMut};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::io::{BufReader, Cursor, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

pub fn keep_alive(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut query: Query<Entity, With<connection_state::Playing>>,
) {
    for entity in &mut query {
        packet_event_emitter
            .send(SendPacketEvent::new(entity, to_client_packets::KeepAlive {}).unwrap())
    }
}

pub fn disconnecting(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    (mut query, mut other): (
        Query<(Entity, &connection_state::Disconnecting)>,
        Query<(Entity, &PlayerEntityDB), With<connection_state::Playing>>,
    ),
    mut commands: Commands,
) {
    for (entity, state) in &mut query {
        packet_event_emitter.send(
            SendPacketEvent::new(
                entity,
                to_client_packets::KickPacket {
                    reason: state.reason.clone(),
                },
            )
            .unwrap(),
        );
        commands
            .entity(entity)
            .remove::<connection_state::Disconnecting>()
            .insert(connection_state::Invalid);
        // Delete player for other players
        for (other, db) in &mut other {
            let mut list: RwLockWriteGuard<Vec<u32>> = db.visible_entities.write().unwrap();
            if let Some(index) = list.iter().position(|p| *p == entity.index()) {
                list.swap_remove(index);
            }
            packet_event_emitter.send(
                SendPacketEvent::new(
                    other,
                    to_client_packets::DestroyEntityPacket {
                        entity_id: entity.index(),
                    },
                )
                .unwrap(),
            );
        }
    }
}

pub fn chat_message(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut chat_message_event_collector: EventReader<event::ChatMessageEvent>,
    mut query: Query<Entity, With<connection_state::Playing>>,
) {
    let messages = chat_message_event_collector.read().collect::<Vec<_>>();
    for entity in &mut query {
        {
            messages.iter().for_each(|m| {
                packet_event_emitter.send(
                    SendPacketEvent::new(
                        entity,
                        to_client_packets::ChatMessagePacket {
                            message: format!("<{}> {}", m.from, m.message),
                        },
                    )
                    .unwrap(),
                );
            });
        }
    }
}

pub fn system_message(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut chat_message_event_collector: EventReader<event::SystemMessageEvent>,
    mut query: Query<Entity, With<connection_state::Playing>>,
) {
    let messages = chat_message_event_collector.read().collect::<Vec<_>>();
    for entity in &mut query {
        {
            messages.iter().for_each(|m| {
                packet_event_emitter.send(
                    SendPacketEvent::new(
                        entity,
                        to_client_packets::ChatMessagePacket {
                            message: m.message.clone(),
                        },
                    )
                    .unwrap(),
                );
            });
        }
    }
}

pub fn calculate_visible_players(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    (mut query_entities, mut other): (
        Query<(Entity, &mut PlayerEntityDB, &PlayerChunkDB), With<connection_state::Playing>>,
        Query<(Entity, &Position, &Look, &Named), With<connection_state::Playing>>,
    ),
    mut commands: Commands,
) {
    for (entity, player_db, chunk_db) in &mut query_entities {
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
                    packet_event_emitter.send(
                        SendPacketEvent::new(
                            entity,
                            to_client_packets::EntityPacket {
                                entity_id: other.index(),
                            },
                        )
                        .unwrap(),
                    );
                    let (rotation, pitch) = util::pack_float_pair(other_look.yaw, other_look.pitch);
                    packet_event_emitter.send(
                        SendPacketEvent::new(
                            entity,
                            to_client_packets::NamedEntitySpawnPacket {
                                entity_id: other.index(),
                                name: other_name_component.name.clone(),
                                x: (other_position.x * 32.0).round() as i32,
                                y: (other_position.y * 32.0).round() as i32,
                                z: (other_position.z * 32.0).round() as i32,
                                rotation,
                                pitch,
                                current_item: 0,
                            },
                        )
                        .unwrap(),
                    );
                    debug!(
                        "Sent spawn entity: {} to entity: {}",
                        other.index(),
                        entity.index()
                    );
                }
                (false, true) => {
                    let index = list.iter().position(|p| *p == other.index()).unwrap();
                    list.swap_remove(index);
                    packet_event_emitter.send(
                        SendPacketEvent::new(
                            entity,
                            to_client_packets::DestroyEntityPacket {
                                entity_id: other.index(),
                            },
                        )
                        .unwrap(),
                    );
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
    mut event_collector: EventReader<PlayerPositionAndLookEvent>,
    mut query: Query<
        (Entity, &mut Position, &mut PreviousPosition, &mut Look),
        With<connection_state::Playing>,
    >,
) {
    let events = event_collector.read().collect::<Vec<_>>();
    for (entity, mut position, mut prev, mut look) in &mut query {
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
                        prev.x = position.x;
                        prev.y = position.y;
                        prev.z = position.z;
                        prev.stance = position.stance;
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
                        prev.x = position.x;
                        prev.y = position.y;
                        prev.z = position.z;
                        prev.stance = position.stance;
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

pub fn move_player(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    (mut query, mut other): (
        Query<Entity, With<connection_state::Playing>>,
        Query<(Entity, &Position, &PreviousPosition, &Look), With<connection_state::Playing>>,
    ),
) {
    for entity in &mut query {
        for (other, position, prev_position, look) in &mut other {
            if entity.index() == other.index() {
                continue;
            }

            let (yaw, pitch) = crate::util::pack_float_pair(look.yaw, look.pitch);
            if prev_position.distance_moved(&position) < 4.0 {
                let (x, y, z) = prev_position.relative_movement(&position);
                packet_event_emitter.send(
                    SendPacketEvent::new(
                        entity,
                        to_client_packets::EntityLookRelativeMovePacket {
                            entity_id: other.index(),
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        },
                    )
                    .unwrap(),
                );
            } else {
                packet_event_emitter.send(
                    SendPacketEvent::new(
                        entity,
                        to_client_packets::EntityTeleportPacket {
                            entity_id: other.index(),
                            x: (position.x * 32.0).round() as i32,
                            y: (position.y * 32.0).round() as i32,
                            z: (position.z * 32.0).round() as i32,
                            yaw,
                            pitch,
                        },
                    )
                    .unwrap(),
                );
            }
        }
    }
}

pub fn correct_player_position(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    (mut query, mut other): (
        Query<Entity, With<connection_state::Playing>>,
        Query<(Entity, &Position, &Look), With<connection_state::Playing>>,
    ),
) {
    for entity in &mut query {
        for (other, position, look) in &mut other {
            if entity.index() == other.index() {
                continue;
            }

            let (yaw, pitch) = crate::util::pack_float_pair(look.yaw, look.pitch);

            packet_event_emitter.send(
                SendPacketEvent::new(
                    entity,
                    to_client_packets::EntityTeleportPacket {
                        entity_id: other.index(),
                        x: (position.x * 32.0).round() as i32,
                        y: (position.y * 32.0).round() as i32,
                        z: (position.z * 32.0).round() as i32,
                        yaw,
                        pitch,
                    },
                )
                .unwrap(),
            );
        }
    }
}

pub fn digging(
    mut event_collector: EventReader<PlayerDiggingEvent>,
    mut event_emitter: EventWriter<BlockChangeEvent>,
    mut query: Query<(Entity, &Digging), With<Digging>>,
    mut commands: Commands,
) {
    for event in event_collector.read() {
        debug!("{event:?}");
        match event {
            PlayerDiggingEvent::Started {
                entity,
                x,
                y,
                z,
                face,
            } => {
                commands.entity(*entity).insert(Digging {
                    x: *x,
                    y: *y,
                    z: *z,
                    face: *face,
                });
            }
            PlayerDiggingEvent::InProgress { entity } => {}
            PlayerDiggingEvent::Stopped { entity } => {
                commands.entity(*entity).remove::<Digging>();
            }
            PlayerDiggingEvent::Completed { entity } => {
                for (player, digging) in &mut query {
                    if player.index() != entity.index() {
                        continue;
                    }
                    event_emitter.send(BlockChangeEvent {
                        x: digging.x,
                        y: digging.y,
                        z: digging.z,
                        ty: 0,
                        metadata: 0,
                    });
                }
                commands.entity(*entity).remove::<Digging>();
            }
        }
    }
}

pub fn block_change(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut world: ResMut<World>,
    mut event_collector: EventReader<BlockChangeEvent>,
    mut query: Query<Entity, With<connection_state::Playing>>,
) {
    let events = event_collector.read().collect::<Vec<_>>();
    for entity in &mut query {
        for event in events.clone() {
            let (chunk_x, chunk_z) = (event.x >> 4, event.z >> 4);
            if let Ok(chunk) = world.get_chunk(chunk_x, chunk_z) {
                if let Ok(mut chunk) = chunk.write() {
                    let old_block = chunk.set_block(
                        (event.x & 15) as u8,
                        event.y as u8,
                        (event.z & 15) as u8,
                        0,
                    );
                    info!("Removed block from ExampleWorld: {old_block:?}");
                } else {
                    warn!("Cloud not obtain chunk!")
                }
            } else {
                warn!("Chunk is unable to load!")
            }
            packet_event_emitter.send(
                SendPacketEvent::new(
                    entity,
                    to_client_packets::BlockChangePacket {
                        x: event.x,
                        y: event.y,
                        z: event.z,
                        block_type: event.ty as i8,
                        block_metadata: event.metadata as i8,
                    },
                )
                .unwrap(),
            );
        }
    }
}

pub fn animation(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut event_collector: EventReader<AnimationEvent>,
    mut query: Query<Entity, With<connection_state::Playing>>,
) {
    let animations = event_collector.read().collect::<Vec<_>>();
    for entity in &mut query {
        animations
            .iter()
            .filter(|e| e.entity != entity)
            .map(|e| to_client_packets::AnimationPacket {
                entity_id: e.entity.index(),
                animate: e.animation,
            })
            .for_each(|p| packet_event_emitter.send(SendPacketEvent::new(entity, p).unwrap()));
    }
}

pub fn player_use(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut event_collector: EventReader<PlayerUseEvent>,
    mut query: Query<Entity, With<connection_state::Playing>>,
) {
    let events = event_collector.read().collect::<Vec<_>>();
    for entity in &mut query {
        for event in events.clone() {
            packet_event_emitter.send(
                SendPacketEvent::new(
                    entity,
                    to_client_packets::EntityVelocityPacket {
                        entity_id: event.target.index(),
                        vel_x: 0,
                        vel_y: i16::MAX,
                        vel_z: 0,
                    },
                )
                .unwrap(),
            );
        }
    }
}

pub fn load_chunks(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut world: ResMut<World>,
    mut query: Query<(Entity, &Position, &mut PlayerChunkDB), With<connection_state::Playing>>,
) {
    for (entity, position, mut db) in &mut query {
        // Get players chunk
        let x = position.x.floor() as i32;
        let z = position.z.floor() as i32;
        let (player_chunk_x, player_chunk_z) = (x >> 4, z >> 4);

        let chunk_r = crate::RENDER_DISTANCE_RADIUS;
        for x in (player_chunk_x - chunk_r)..=(player_chunk_x + chunk_r) {
            for z in (player_chunk_z - chunk_r)..=(player_chunk_z + chunk_r) {
                if db.chunks.get(&(x, z)).is_none() {
                    match world.get_chunk(x, z) {
                        Ok(chunk) => {
                            debug!("Loaded chunk at (x: {x}, z: {z}).");
                            if db.chunks.insert((x, z), chunk.clone()).is_none() {
                                let packet = SendPacketEvent::with_ord(
                                    entity,
                                    1,
                                    to_client_packets::PreChunkPacket { x, z, mode: true },
                                )
                                .unwrap();
                                packet_event_emitter.send(packet);
                                let (len, chunk_data) = chunk.read().unwrap().get_compressed_data();
                                let packet = SendPacketEvent::with_ord(
                                    entity,
                                    2,
                                    to_client_packets::MapChunkPacket {
                                        x: x * 16,
                                        y: 0,
                                        z: z * 16,
                                        size_x: 15,
                                        size_y: 127,
                                        size_z: 15,
                                        compressed_size: len,
                                        compressed_data: chunk_data[..len as usize].to_vec(),
                                    },
                                )
                                .unwrap();
                                packet_event_emitter.send(packet);
                            }
                        }
                        Err(err) => {
                            error!("Failed to load chunk at (x: {x}, z: {z}): {err}!")
                        }
                    }
                }
            }
        }
    }
}

pub fn unload_chunks(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut world: ResMut<World>,
    mut query: Query<(Entity, &Position, &mut PlayerChunkDB), With<connection_state::Playing>>,
) {
    for (entity, position, mut db) in &mut query {
        // Get players chunk
        let x = position.x.floor() as i32;
        let z = position.z.floor() as i32;
        let (player_chunk_x, player_chunk_z) = (x >> 4, z >> 4);

        let chunk_r = crate::RENDER_DISTANCE_RADIUS * 2; // Buffer zone
        let mut allowed_chunks = Vec::with_capacity(chunk_r as usize * chunk_r as usize);
        for x in (player_chunk_x - chunk_r)..=(player_chunk_x + chunk_r) {
            for z in (player_chunk_z - chunk_r)..=(player_chunk_z + chunk_r) {
                allowed_chunks.push((x, z));
            }
        }
        let to_remove = db
            .chunks
            .keys()
            .filter(|k| !allowed_chunks.contains(*k))
            .copied()
            .collect::<Vec<_>>();
        for (x, z) in to_remove {
            debug!("Unloaded chunk at (x: {x}, z: {z}).");
            if db.chunks.remove(&(x, z)).is_some() {
                let _ = world.unload_chunk(x, z);
            }
            packet_event_emitter.send(
                SendPacketEvent::with_ord(
                    entity,
                    3,
                    to_client_packets::PreChunkPacket { x, z, mode: false },
                )
                .unwrap(),
            );
        }
    }
}

pub fn increment_time(
    mut packet_event_emitter: EventWriter<SendPacketEvent>,
    mut world: ResMut<World>,
    mut query: Query<Entity, With<connection_state::Playing>>,
) {
    let current_time = world.get_time();
    world.set_time(current_time + 20);
    let packet = to_client_packets::TimeUpdatePacket {
        time: world.get_time(),
    };
    for entity in &mut query {
        packet_event_emitter.send(SendPacketEvent::new(entity, packet.clone()).unwrap());
    }
}
