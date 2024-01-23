use crate::entity::connection_state::Login;
use crate::entity::{ClientStream, PlayerBundle};
use crate::packet::to_server_packets;
use crate::world::World;
use bevy::ecs::schedule::ExecutorKind;
use bevy::prelude::{App, Resource, Schedule, Update};
use log::{debug, info, Level};
use std::net::TcpListener;
use std::time::Instant;

mod byte_man;
mod entity;
mod event;
mod packet;
mod system;
mod util;
mod world;

pub(crate) const BUFFER_SIZE: usize = 1024 * 8;
pub(crate) const RENDER_DISTANCE_RADIUS: i32 = 4; // Diameter of chunks to send to player in `Initializing` state.

fn main() -> std::io::Result<()> {
    simple_logger::init_with_level(Level::Debug).expect("Failed to initialize logging!");
    let listener = TcpListener::bind("0.0.0.0:25565")?;
    listener.set_nonblocking(true)?;
    App::new()
        .add_schedule(Schedule::new(schedule::CoreLabel()))
        .add_schedule(Schedule::new(schedule::ServerTickLabel()))
        .add_schedule(Schedule::new(schedule::SecondTickLabel()))
        .add_schedule(Schedule::new(schedule::ChunkLabel()))
        .add_schedule(Schedule::new(schedule::AfterTickLabel()))
        .add_event::<event::SendPacketEvent>()
        .add_event::<event::ChatMessageEvent>()
        .add_event::<event::PlayerPositionAndLookEvent>()
        .add_event::<event::SystemMessageEvent>()
        .add_event::<event::PlayerDiggingEvent>()
        .add_event::<event::BlockChangeEvent>()
        .add_systems(
            schedule::CoreLabel(),
            (
                core::accept_system,
                core::login_system,
                core::initializing_system,
                core::event_emitter_system,
            ),
        )
        // TODO: Chunks need to be loaded more async, because loading and unloading them causes lag.
        .add_systems(
            schedule::ChunkLabel(),
            (system::load_chunks, system::unload_chunks),
        )
        .add_systems(
            schedule::ServerTickLabel(),
            (
                system::keep_alive,
                system::chat_message,
                system::system_message,
                system::disconnecting,
                system::digging,
                system::block_change,
                system::calculate_visible_players,
                system::correct_player_position,
                system::player_movement,
                system::move_player,
            ),
        )
        .add_systems(
            schedule::AfterTickLabel(),
            (core::send_packets_system, core::remove_invalid_players),
        )
        //.add_systems(schedule::SecondTickLabel(), (system::increment_time,))
        .edit_schedule(schedule::CoreLabel(), |s| {
            s.set_executor_kind(ExecutorKind::MultiThreaded);
        })
        .edit_schedule(schedule::ServerTickLabel(), |s| {
            s.set_executor_kind(ExecutorKind::MultiThreaded);
        })
        .edit_schedule(schedule::ChunkLabel(), |s| {
            s.set_executor_kind(ExecutorKind::MultiThreaded);
        })
        .insert_resource(World::open("./ExampleWorld")?)
        .insert_resource(TcpWrapper { listener })
        .set_runner(|mut app: App| {
            let mut instant = Instant::now();
            let mut second_instant = Instant::now();
            loop {
                app.world.run_schedule(schedule::CoreLabel());
                app.world.run_schedule(schedule::ChunkLabel());
                if instant.elapsed().as_millis() >= 50 {
                    app.world.run_schedule(schedule::ServerTickLabel());
                    instant = Instant::now();
                }
                if second_instant.elapsed().as_millis() >= 1000 {
                    app.world.run_schedule(schedule::SecondTickLabel());
                    second_instant = Instant::now();
                }
                app.world.run_schedule(schedule::AfterTickLabel());
            }
        })
        .run();
    Ok(())
}

#[derive(Resource)]
struct TcpWrapper {
    pub listener: TcpListener,
}

mod core {
    use crate::byte_man::{get_string, get_u8};
    use crate::entity::{connection_state, Position};
    use crate::entity::{
        ClientStream, Look, Named, PlayerBundle, PlayerChunkDB, PlayerEntityDB, PreviousPosition,
        Velocity,
    };
    use crate::packet::{ids, to_client_packets, to_server_packets, PacketError};
    use crate::packet::{Deserialize, Serialize};
    use crate::world::{Chunk, World};
    use crate::{event, packet, util, TcpWrapper, BUFFER_SIZE};
    use bevy::prelude::{
        Commands, Entity, EventReader, EventWriter, Mut, Query, Res, ResMut, With,
    };
    use bytes::{Buf, BufMut, BytesMut};
    use log::{debug, error, info, warn};
    use std::collections::HashMap;
    use std::io::{BufReader, Cursor, ErrorKind, Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, RwLock, RwLockWriteGuard};

    pub fn accept_system(wrapper: Res<TcpWrapper>, mut commands: Commands) {
        if let Ok((mut stream, addr)) = wrapper.listener.accept() {
            info!("Got new connection {}", stream.peer_addr().unwrap());
            stream.set_nonblocking(true).unwrap();
            // Create the player entity
            commands.spawn((ClientStream::new(stream), connection_state::Login));
        }
    }

    pub fn login_system(
        world: Res<World>,
        mut query: Query<(Entity, &ClientStream), With<connection_state::Login>>,
        mut commands: Commands,
    ) {
        #[derive(PartialEq)]
        enum InternalState {
            LoggingIn,
            LoggedIn,
        }
        for (entity, stream) in &mut query {
            {
                let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
                let mut buf = [0u8; BUFFER_SIZE];
                let (mut buf_start, mut buf_end) = (0usize, 0usize);
                let mut state = InternalState::LoggingIn;
                loop {
                    fn handle_packets<'w, 's>(
                        stream: &mut TcpStream,
                        buf: &[u8],
                        entity: Entity,
                        world: &World,
                        commands: &mut Commands<'w, 's>,
                        state: &mut InternalState,
                    ) -> Result<usize, PacketError> {
                        let mut cursor = Cursor::new(buf);
                        while let Ok(packet_id) = get_u8(&mut cursor) {
                            match packet_id {
                                ids::KEEP_ALIVE => {
                                    to_server_packets::HandshakePacket::nested_deserialize(
                                        &mut cursor,
                                    )?;
                                    stream
                                        .write_all(&to_client_packets::KeepAlive {}.serialize()?)
                                        .unwrap();
                                    stream.flush().unwrap();
                                }
                                ids::HANDSHAKE => {
                                    let name =
                                        to_server_packets::HandshakePacket::nested_deserialize(
                                            &mut cursor,
                                        )?;
                                    debug!(
                                        "Received handshake with name {:?}",
                                        name.connection_hash
                                    );
                                    let packet = to_client_packets::HandshakePacket {
                                        connection_hash: "-".to_string(),
                                    };
                                    stream.write_all(&packet.serialize().unwrap()).unwrap();
                                    stream.flush().unwrap();
                                    debug!("Handshake accepted from address {:?} using username {name:?}", stream.peer_addr().unwrap())
                                }
                                ids::LOGIN => {
                                    let request =
                                        to_server_packets::LoginRequestPacket::nested_deserialize(
                                            &mut cursor,
                                        )?;
                                    commands.entity(entity).insert(Named {
                                        name: request.username.clone(),
                                    });
                                    debug!("Received login request from address {:?} containing {request:?}", stream.peer_addr().unwrap());
                                    let response = to_client_packets::LoginResponsePacket {
                                        entity_id: entity.index(),
                                        _unused1: "".to_string(),
                                        _unused2: "".to_string(),
                                        map_seed: world.get_seed(),
                                        dimension: 0,
                                    };
                                    stream.write_all(&response.serialize()?).unwrap();
                                    stream.flush().unwrap();
                                    info!("Player \"{}\" joined the server!", request.username);
                                    *state = InternalState::LoggedIn;
                                }
                                _ => {
                                    error!("Unhandled packet id: {packet_id}");
                                    return Err(PacketError::InvalidPacketID(packet_id));
                                }
                            }
                        }
                        Ok(cursor.position() as usize)
                    }

                    if let Ok(n) = handle_packets(
                        &mut stream,
                        &buf[buf_start..buf_end],
                        entity,
                        &world,
                        &mut commands,
                        &mut state,
                    ) {
                        buf_start += n;
                    }

                    match stream.read(&mut buf[buf_end..]) {
                        Ok(0) => {
                            debug!("Read zero bytes...");
                            break;
                        }
                        Ok(n) => {
                            buf_end += n;
                        }
                        _ => {}
                    }

                    if state == InternalState::LoggedIn {
                        break;
                    }
                }
            }
            // Transition state from `Login` to `Initializing`
            commands
                .entity(entity)
                .remove::<connection_state::Login>()
                .insert(connection_state::Initializing {});
        }
    }

    // TODO: Parse spawn position as absolute integer.
    pub fn initializing_system(
        mut world: ResMut<World>,
        mut query: Query<(Entity, &ClientStream, &Named), With<connection_state::Initializing>>,
        mut commands: Commands,
    ) {
        for (entity, stream, name_component) in &mut query {
            {
                let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
                // Send chunk data
                let (player_chunk_x, player_chunk_z) = (
                    (world.get_spawn()[0] - world.get_spawn()[0] % 16) / 16,
                    (world.get_spawn()[2] - world.get_spawn()[2] % 16) / 16,
                );
                debug!(
                    "Player {} spawned in chunk: [{player_chunk_x}, {player_chunk_z}].",
                    name_component.name
                );
                let mut local_db = HashMap::with_capacity(8 * 8);
                let chunk_r = crate::RENDER_DISTANCE_RADIUS / 2;
                for x in (player_chunk_x - chunk_r)..=(player_chunk_x + chunk_r) {
                    for z in (player_chunk_z - chunk_r)..=(player_chunk_z + chunk_r) {
                        match world.get_chunk(x, z) {
                            Ok(chunk) => {
                                debug!("Loaded chunk at (x: {x}, z: {z}).");
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
                                local_db.insert((x, z), chunk);
                            }
                            Err(err) => {
                                warn!("Failed to load chunk at (x: {x}, z: {z}): {err}!")
                            }
                        }
                    }
                }
                stream.flush().unwrap();
                info!("Sent chunk data to {}.", name_component.name);
                commands
                    .entity(entity)
                    .insert(PlayerChunkDB { chunks: local_db });
                // Send spawn information
                let spawn_packet = to_client_packets::SpawnPositionPacket {
                    x: world.get_spawn()[0],
                    y: world.get_spawn()[1],
                    z: world.get_spawn()[2],
                };
                match spawn_packet.serialize() {
                    Ok(data) => {
                        stream.write_all(&data).unwrap();
                        stream.flush().unwrap();
                        info!(
                            "Sent spawn position {:?} to player: {}.",
                            world.get_spawn(),
                            name_component.name
                        )
                    }
                    Err(err) => {
                        error!("Failed to send SpawnPacket: {err}!")
                    }
                }
                // TODO: Add spawn component to player.
                // Send position and look information
                // TODO: Load position and look information from player file.
                let position_and_look_packet = to_client_packets::ServerPositionLookPacket {
                    x: world.get_spawn()[0] as f64,
                    stance: world.get_spawn()[1] as f64 + 1.75,
                    y: world.get_spawn()[1] as f64,
                    z: world.get_spawn()[2] as f64,
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: false,
                };
                stream
                    .write_all(&position_and_look_packet.serialize().unwrap())
                    .unwrap();
                stream.flush().unwrap();
                commands.entity(entity).insert((
                    Position {
                        x: world.get_spawn()[0] as f64,
                        y: world.get_spawn()[1] as f64,
                        z: world.get_spawn()[2] as f64,
                        stance: world.get_spawn()[1] as f64 + 1.65,
                        on_ground: false,
                    },
                    PreviousPosition {
                        x: world.get_spawn()[0] as f64,
                        y: world.get_spawn()[1] as f64,
                        z: world.get_spawn()[2] as f64,
                        stance: world.get_spawn()[1] as f64 + 1.65,
                        on_ground: false,
                    },
                    // Velocity {
                    //     x: 0.0,
                    //     y: 0.0,
                    //     z: 0.0,
                    // },
                    Look {
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    PlayerEntityDB {
                        visible_entities: Arc::new(RwLock::new(Vec::new())),
                    },
                ));
            }
            // Transition state from `Initializing` to `Playing`
            info!("{} joined the world!", name_component.name);
            commands
                .entity(entity)
                .remove::<connection_state::Initializing>()
                .insert(connection_state::Playing {});
        }
    }

    pub fn send_packets_system(
        mut packet_send_collector: EventReader<event::SendPacketEvent>,
        mut query: Query<(Entity, &ClientStream)>,
    ) {
        let mut packets_to_send = packet_send_collector.read().collect::<Vec<_>>();
        packets_to_send.sort();
        for (entity, stream_component) in &mut query {
            let mut stream: RwLockWriteGuard<'_, TcpStream> =
                stream_component.stream.write().unwrap();
            // Send Packets
            packets_to_send
                .iter()
                .filter(|p| p.entity == entity)
                .for_each(|p| {
                    stream.write_all(&p.bytes).unwrap();
                });
            stream.flush().unwrap();
        }
    }

    pub fn remove_invalid_players(
        mut query: Query<Entity, With<connection_state::Invalid>>,
        mut commands: Commands,
    ) {
        for entity in &mut query {
            commands.entity(entity).despawn();
        }
    }

    // This is the dirty part no one wants to talk about.
    pub fn event_emitter_system(
        mut system_message_event_emitter: EventWriter<event::SystemMessageEvent>,
        mut chat_message_event_emitter: EventWriter<event::ChatMessageEvent>,
        mut position_and_look_event_emitter: EventWriter<event::PlayerPositionAndLookEvent>,
        mut player_digging_event_emitter: EventWriter<event::PlayerDiggingEvent>,
        mut query: Query<(Entity, &ClientStream, &Named), (With<connection_state::Playing>)>,
        mut commands: Commands,
    ) {
        for (entity, stream_component, name_component) in &mut query {
            let mut stream: RwLockWriteGuard<'_, TcpStream> =
                stream_component.stream.write().unwrap();
            // This buffer has to be persistent between read cycles, because we cannot read the exact number of bytes we need.
            let mut buf = [0u8; BUFFER_SIZE];
            let mut left_over: RwLockWriteGuard<'_, Vec<u8>> =
                stream_component.left_over.write().unwrap();
            unsafe {
                std::ptr::copy_nonoverlapping(left_over.as_ptr(), buf.as_mut_ptr(), left_over.len())
            }
            // debug!(
            //     "Current backlog for entity {} is {}b contains {:?}",
            //     entity.index(),
            //     left_over.len(),
            //     left_over
            // );
            let (mut buf_start, mut buf_end) = (0usize, left_over.len());
            left_over.clear();

            match stream.read(&mut buf[buf_end..]) {
                Ok(0) => {
                    debug!("Read zero bytes...");
                }
                Ok(n) => {
                    buf_end += n;
                }
                Err(err) => match err.kind() {
                    ErrorKind::ConnectionRefused
                    | ErrorKind::ConnectionReset
                    | ErrorKind::BrokenPipe
                    | ErrorKind::TimedOut => {
                        // Transition state from `Playing` to `Disconnecting`
                        info!(
                            "{} left the world, because of error {err}",
                            name_component.name
                        );
                        commands
                            .entity(entity)
                            .remove::<connection_state::Playing>()
                            .insert(connection_state::Disconnecting {
                                reason: "Broke!".to_string(),
                            });
                    }
                    ErrorKind::WouldBlock => {}
                    _ => {
                        error!("{err}");
                    }
                },
            }

            let res: Result<usize, PacketError> = (|| -> Result<usize, PacketError> {
                let mut cursor = Cursor::new(&buf[buf_start..buf_end]);
                // Handle all packets...
                while let Ok(packet_id) = get_u8(&mut cursor) {
                    match packet_id {
                        ids::KEEP_ALIVE => {
                            to_server_packets::HandshakePacket::nested_deserialize(&mut cursor)?;
                        }
                        ids::HANDSHAKE => {
                            let packet = to_server_packets::HandshakePacket::nested_deserialize(
                                &mut cursor,
                            )?;
                            warn!("Received invalid handshake packet: {packet:?}")
                        }
                        ids::LOGIN => {
                            let packet = to_server_packets::LoginRequestPacket::nested_deserialize(
                                &mut cursor,
                            )?;
                            warn!("Received invalid login packet: {packet:?}")
                        }
                        ids::CHAT_MESSAGE => {
                            let packet = to_server_packets::ChatMessagePacket::nested_deserialize(
                                &mut cursor,
                            )?;
                            chat_message_event_emitter.send(event::ChatMessageEvent {
                                from: name_component.name.clone(),
                                message: packet.message,
                            });
                        }
                        ids::PLAYER_POSITION_AND_LOOK => {
                            let to_server_packets::PlayerPositionLookPacket {
                                x,
                                y,
                                stance,
                                z,
                                yaw,
                                pitch,
                                on_ground,
                            } = to_server_packets::PlayerPositionLookPacket::nested_deserialize(
                                &mut cursor,
                            )?;
                            position_and_look_event_emitter.send(
                                event::PlayerPositionAndLookEvent::PositionAndLook {
                                    entity_id: entity.index(),
                                    x,
                                    y,
                                    z,
                                    stance,
                                    yaw,
                                    pitch,
                                },
                            );
                        }
                        ids::PLAYER => {
                            let packet =
                                to_server_packets::PlayerPacket::nested_deserialize(&mut cursor)?;
                        }
                        ids::PLAYER_POSITION => {
                            let to_server_packets::PlayerPositionPacket {
                                x,
                                y,
                                stance,
                                z,
                                on_ground,
                            } = to_server_packets::PlayerPositionPacket::nested_deserialize(
                                &mut cursor,
                            )?;
                            position_and_look_event_emitter.send(
                                event::PlayerPositionAndLookEvent::Position {
                                    entity_id: entity.index(),
                                    x,
                                    y,
                                    z,
                                    stance,
                                },
                            );
                        }
                        ids::PLAYER_LOOK => {
                            let to_server_packets::PlayerLookPacket {
                                yaw,
                                pitch,
                                on_ground,
                            } = to_server_packets::PlayerLookPacket::nested_deserialize(
                                &mut cursor,
                            )?;
                            position_and_look_event_emitter.send(
                                event::PlayerPositionAndLookEvent::Look {
                                    entity_id: entity.index(),
                                    yaw,
                                    pitch,
                                },
                            );
                        }
                        ids::ANIMATION => {
                            let packet = to_server_packets::ArmAnimationPacket::nested_deserialize(
                                &mut cursor,
                            )?;
                        }
                        ids::PLAYER_DIGGING => {
                            let packet =
                                to_server_packets::PlayerDiggingPacket::nested_deserialize(
                                    &mut cursor,
                                )?;
                            // debug!("{packet:?}");
                            let event = match packet.status {
                                0 => Some(event::PlayerDiggingEvent::Started {
                                    entity,
                                    x: packet.x,
                                    y: packet.y,
                                    z: packet.z,
                                    face: event::Face::from(packet.face),
                                }),
                                1 => Some(event::PlayerDiggingEvent::InProgress { entity }),
                                2 => Some(event::PlayerDiggingEvent::Stopped { entity }),
                                3 => Some(event::PlayerDiggingEvent::Completed { entity }),
                                _ => {
                                    warn!("Recieved unknown digging status: {}", packet.status);
                                    None
                                }
                            };
                            if let Some(event) = event {
                                player_digging_event_emitter.send(event)
                            }
                        }
                        ids::KICK_OR_DISCONNECT => {
                            let packet = to_server_packets::DisconnectPacket::nested_deserialize(
                                &mut cursor,
                            )?;
                            info!("{} left the world: {}", name_component.name, packet.reason);
                            system_message_event_emitter.send(event::SystemMessageEvent {
                                message: format!(
                                    "{} left the world [{:?}]",
                                    name_component.name, packet.reason
                                ),
                            });
                            commands
                                .entity(entity)
                                .remove::<connection_state::Playing>()
                                .insert(connection_state::Disconnecting {
                                    reason: packet.reason,
                                });
                        }
                        _ => {
                            error!("Unhandled packet id: {packet_id} cannot continue!");
                            return Err(PacketError::InvalidPacketID(packet_id));
                        }
                    }
                }
                Ok(cursor.position() as usize)
                // else {
                //     Err(PacketError::NotEnoughBytes)
                // }
            })();

            match res {
                Ok(n) => {
                    buf_start += n;
                    left_over.append(&mut buf[buf_start..buf_end].to_vec());
                }
                Err(PacketError::InvalidPacketID(id)) => {
                    commands
                        .entity(entity)
                        .remove::<connection_state::Playing>()
                        .insert(connection_state::Disconnecting {
                            reason: format!(
                                "You send a packet with id: {id}, which isn't handled just yet!"
                            ),
                        });
                }
                Err(..) => {
                    left_over.append(&mut buf[buf_start..buf_end].to_vec());
                }
            }
        }
    }
}

mod schedule {
    use bevy::ecs::schedule::ScheduleLabel;

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct CoreLabel();

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ChunkLabel();

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ServerTickLabel();

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct SecondTickLabel();

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct AfterTickLabel();
}
