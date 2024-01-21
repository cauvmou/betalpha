use std::net::TcpListener;
use std::time::Instant;
use bevy::prelude::{App, Resource, Schedule, Update};
use log::{debug, info, Level};
use crate::entity::{ClientStream, PlayerBundle};
use crate::entity::connection_state::Login;
use crate::packet::to_server_packets;
use crate::world::World;

mod util;
mod world;
mod entity;
mod packet;
mod byte_man;
mod event;

pub(crate) const BUFFER_SIZE: usize = 1024 * 8;
pub(crate) const INITIAL_CHUNK_LOAD_SIZE: i32 = 12; // Diameter of chunks to send to player in `Initializing` state.


fn main() -> std::io::Result<()> {
    simple_logger::init_with_level(Level::Debug).expect("Failed to initialize logging!");
    let listener = TcpListener::bind("0.0.0.0:25565")?;
    listener.set_nonblocking(true)?;
    App::new()
        .add_schedule(Schedule::new(schedule::ServerTickLabel()))
        .add_event::<event::ChatMessageEvent>()
        .add_systems(Update, (
            system::accept_system,
            system::login_system,
            system::initializing_system,
            system::event_emitter_system,
        ))
        .add_systems(schedule::ServerTickLabel(),
                     (
                         system::keep_alive_system,
                         system::chat_message_system,
                         system::disconnecting_system,
                     ),
        )
        .insert_resource(World::open("./ExampleWorld")?)
        .insert_resource(TcpWrapper { listener })
        .set_runner(|mut app: App| {
            let mut instant = Instant::now();
            loop {
                app.update();
                if instant.elapsed().as_millis() >= 50 {
                    app.world.run_schedule(schedule::ServerTickLabel());
                    instant = Instant::now();
                }
            }
        })
        .run();
    Ok(())
}

#[derive(Resource)]
struct TcpWrapper {
    pub listener: TcpListener,
}

mod system {
    use std::collections::HashMap;
    use std::io::{BufReader, Cursor, ErrorKind, Read, Write};
    use std::net::TcpStream;
    use std::sync::RwLockWriteGuard;
    use bevy::prelude::{Commands, Entity, EventReader, EventWriter, Query, Res, ResMut, With};
    use bytes::{Buf, BufMut, BytesMut};
    use log::{debug, error, info, warn};
    use crate::{BUFFER_SIZE, event, packet, TcpWrapper};
    use crate::byte_man::{get_string, get_u8};
    use crate::entity::{ClientStream, Look, Named, PlayerBundle, PlayerChunkDB, Position, Velocity};
    use crate::entity::connection_state;
    use crate::packet::{ids, PacketError, to_client_packets, to_server_packets};
    use crate::packet::{Deserialize, Serialize};
    use crate::world::World;

    pub fn accept_system(
        wrapper: Res<TcpWrapper>,
        mut commands: Commands,
    ) {
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
                pollster::block_on(async {
                    loop {
                        fn handle_packets<'w, 's>(stream: &mut TcpStream, buf: &[u8], entity: Entity, world: &World, commands: &mut Commands<'w, 's>, state: &mut InternalState) -> Result<usize, PacketError> {
                            let mut cursor = Cursor::new(buf);
                            while let Ok(packet_id) = get_u8(&mut cursor) {
                                match packet_id {
                                    ids::KEEP_ALIVE => {
                                        to_server_packets::HandshakePacket::nested_deserialize(&mut cursor)?;
                                        stream.write_all(&to_client_packets::KeepAlive {}.serialize()?).unwrap();
                                        stream.flush().unwrap();
                                    }
                                    ids::HANDSHAKE => {
                                        let name = to_server_packets::HandshakePacket::nested_deserialize(&mut cursor)?;
                                        debug!("Received handshake with name {:?}", name.connection_hash);
                                        let packet = to_client_packets::HandshakePacket {
                                            connection_hash: "-".to_string(),
                                        };
                                        stream.write_all(&packet.serialize().unwrap()).unwrap();
                                        stream.flush().unwrap();
                                        debug!("Handshake accepted from address {:?} using username {name:?}", stream.peer_addr().unwrap())
                                    }
                                    ids::LOGIN => {
                                        let request = to_server_packets::LoginRequestPacket::nested_deserialize(&mut cursor)?;
                                        commands.entity(entity).insert(Named { name: request.username.clone() });
                                        debug!("Received login request from address {:?} containing {request:?}", stream.peer_addr().unwrap());
                                        let response = to_client_packets::LoginResponsePacket {
                                            entity_id: entity.index() as i32,
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

                        if let Ok(n) = handle_packets(&mut stream, &buf[buf_start..buf_end], entity, &world, &mut commands, &mut state) {
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
                });
            }
            // Transition state from `Login` to `Initializing`
            commands.entity(entity).remove::<connection_state::Login>().insert(connection_state::Initializing {});
        }
    }

    pub fn initializing_system(
        mut world: ResMut<World>,
        mut query: Query<(Entity, &ClientStream, &Named), With<connection_state::Initializing>>,
        mut commands: Commands,
    ) {
        for (entity, stream, name_component) in &mut query {
            {
                let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
                // Send chunk data
                let (player_chunk_x, player_chunk_z) = ((world.get_spawn()[0] - world.get_spawn()[0] % 16) / 16, (world.get_spawn()[2] - world.get_spawn()[2] % 16) / 16);
                debug!("Player {} is in spawned in chunk: [{player_chunk_x}, {player_chunk_z}].", name_component.name);
                let mut local_db = HashMap::with_capacity(8 * 8);
                let chunk_r = crate::INITIAL_CHUNK_LOAD_SIZE / 2;
                for x in (player_chunk_x - chunk_r)..(player_chunk_x + chunk_r) {
                    for z in (player_chunk_z - chunk_r)..(player_chunk_z + chunk_r) {
                        match world.get_chunk(x, z) {
                            Ok(chunk) => {
                                debug!("Loaded chunk at (x: {x}, z: {z}).");
                                stream.write_all(&to_client_packets::PreChunkPacket {
                                    x,
                                    z,
                                    mode: true,
                                }.serialize().unwrap()).unwrap();

                                let (len, chunk_data) = chunk.read().unwrap().get_compressed_data();

                                stream.write_all(&to_client_packets::MapChunkPacket {
                                    x: x * 16,
                                    y: 0,
                                    z: z * 16,
                                    size_x: 15,
                                    size_y: 127,
                                    size_z: 15,
                                    compressed_size: len,
                                    compressed_data: chunk_data[..len as usize].to_vec(),
                                }.serialize().unwrap()).unwrap();
                                let key = (x as u64) << 4 | z as u64;
                                local_db.insert(key, chunk);
                            }
                            Err(err) => {
                                error!("Failed to load chunk at (x: {x}, z: {z}): {err}!")
                            }
                        }
                    }
                }
                stream.flush().unwrap();
                info!("Sent chunk data to {}.", name_component.name);
                commands.entity(entity).insert(PlayerChunkDB { chunks: local_db });
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
                        info!("Sent spawn position {:?} to player: {}.", world.get_spawn(), name_component.name)
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
                    stance: world.get_spawn()[1] as f64 + 1.75 + 30.0,
                    y: world.get_spawn()[1] as f64 + 30.0,
                    z: world.get_spawn()[2] as f64,
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: false,
                };
                stream.write_all(&position_and_look_packet.serialize().unwrap()).unwrap();
                stream.flush().unwrap();
                commands.entity(entity).insert((
                    Position {
                        x: world.get_spawn()[0] as f64,
                        y: world.get_spawn()[1] as f64,
                        z: world.get_spawn()[2] as f64,
                        stance: world.get_spawn()[1] as f64 + 1.75,
                        on_ground: false,
                    },
                    Velocity {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Look {
                        yaw: 0.0,
                        pitch: 0.0,
                    }
                ));
            }
            // Transition state from `Initializing` to `Playing`
            info!("{} joined the world!", name_component.name);
            commands.entity(entity).remove::<connection_state::Initializing>().insert(connection_state::Playing {});
        }
    }

    pub fn keep_alive_system(mut query: Query<&ClientStream, With<connection_state::Playing>>) {
        for stream in &mut query {
            let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
            stream.write_all(&[0x00]).unwrap();
            stream.flush().unwrap();
        }
    }

    pub fn disconnecting_system(mut query: Query<(Entity, &ClientStream, &connection_state::Disconnecting)>, mut commands: Commands) {
        for (entity, stream, state) in &mut query {
            let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
            let packet = to_client_packets::KickPacket { reason: state.reason.clone() };
            stream.write_all(&packet.serialize().unwrap()).unwrap();
            stream.flush().unwrap();
            commands.entity(entity).despawn();
        }
    }


    pub fn tick_system(
        mut query: Query<(Entity, &ClientStream), With<connection_state::Playing>>
    ) {
        for (entity, stream) in &mut query {}
    }

    pub fn chat_message_system(
        mut chat_message_event_collector: EventReader<event::ChatMessageEvent>,
        mut query: Query<&ClientStream, With<connection_state::Playing>>,
    ) {
        let messages = chat_message_event_collector.read().collect::<Vec<_>>();
        for stream in &mut query {
            {
                let mut stream: RwLockWriteGuard<'_, TcpStream> = stream.stream.write().unwrap();
                messages.iter().for_each(|m| {
                    let packet = to_client_packets::ChatMessagePacket { message: format!("<{}> {}", m.from, m.message) };
                    stream.write_all(&packet.serialize().unwrap()).unwrap();
                });
                stream.flush().unwrap();
            }
        }
    }

    // This is the dirty part no one wants to talk about.
    pub fn event_emitter_system(
        mut chat_message_event_emitter: EventWriter<event::ChatMessageEvent>,
        mut query: Query<(Entity, &ClientStream, &Named), (With<connection_state::Playing>)>,
        mut commands: Commands,
    ) {
        for (entity, stream_component, name_component) in &mut query {
            let mut stream: RwLockWriteGuard<'_, TcpStream> = stream_component.stream.write().unwrap();
            // This buffer has to be persistent between read cycles, because we cannot read the exact number of bytes we need.
            let mut buf = [0u8; BUFFER_SIZE];
            let mut left_over: RwLockWriteGuard<'_, Vec<u8>> = stream_component.left_over.write().unwrap();
            unsafe { std::ptr::copy_nonoverlapping(left_over.as_ptr(), buf.as_mut_ptr(), left_over.len()) }
            let (mut buf_start, mut buf_end) = (0usize, left_over.len());
            loop {
                let res: Result<usize, PacketError> = (|| -> Result<usize, PacketError> {
                    let mut cursor = Cursor::new(&buf[buf_start..buf_end]);
                    // Handle all packets...
                    if let Ok(packet_id) = get_u8(&mut cursor) {
                        match packet_id {
                            ids::KEEP_ALIVE => {
                                to_server_packets::HandshakePacket::nested_deserialize(&mut cursor)?;
                            },
                            ids::HANDSHAKE => {
                                let packet = to_server_packets::HandshakePacket::nested_deserialize(&mut cursor)?;
                                warn!("Received invalid handshake packet: {packet:?}")
                            },
                            ids::LOGIN => {
                                let packet = to_server_packets::LoginRequestPacket::nested_deserialize(&mut cursor)?;
                                warn!("Received invalid login packet: {packet:?}")
                            }
                            ids::CHAT_MESSAGE => {
                                let packet = to_server_packets::ChatMessagePacket::nested_deserialize(&mut cursor)?;
                                chat_message_event_emitter.send(event::ChatMessageEvent { from: name_component.name.clone(), message: packet.message });
                            }
                            ids::PLAYER_POSITION_AND_LOOK => {
                                let packet = to_server_packets::PlayerPositionLookPacket::nested_deserialize(&mut cursor)?;
                            }
                            ids::PLAYER => {
                                let packet = to_server_packets::PlayerPacket::nested_deserialize(&mut cursor)?;
                            }
                            ids::PLAYER_POSITION => {
                                let packet = to_server_packets::PlayerPositionPacket::nested_deserialize(&mut cursor)?;
                            }
                            ids::PLAYER_LOOK => {
                                let packet = to_server_packets::PlayerLookPacket::nested_deserialize(&mut cursor)?;
                            }
                            ids::ANIMATION => {
                                let packet = to_server_packets::ArmAnimationPacket::nested_deserialize(&mut cursor)?;
                            }
                            ids::KICK_OR_DISCONNECT => {
                                let packet = to_server_packets::DisconnectPacket::nested_deserialize(&mut cursor)?;
                                info!("{} left the world: {}", name_component.name, packet.reason);
                                chat_message_event_emitter.send(event::ChatMessageEvent { from: "SYS".to_string(), message: format!("{} left the world for reason: {:?}", name_component.name, packet.reason) });
                            }
                            _ => {
                                error!("Unhandled packet id: {packet_id} cannot continue!");
                                return Err(PacketError::InvalidPacketID(packet_id));
                            }
                        }
                        Ok(cursor.position() as usize)
                    } else {
                        Err(PacketError::NotEnoughBytes)
                    }
                })();

                match res {
                    Ok(n) => {
                        buf_start += n;
                        left_over.clear();
                        left_over.append(&mut buf[buf_start..buf_end].to_vec());
                        break;
                    }
                    Err(PacketError::InvalidPacketID(id)) => {
                        commands.entity(entity).remove::<connection_state::Playing>().insert(
                            connection_state::Disconnecting { reason: format!("You send a packet with id: {id}, which isn't handled just yet!") }
                        );
                        break;
                    }
                    Err(..) => {}
                }

                match stream.read(&mut buf[buf_end..]) {
                    Ok(0) => {
                        debug!("Read zero bytes...");
                        left_over.clear();
                        left_over.append(&mut buf[buf_start..buf_end].to_vec());
                        break;
                    }
                    Ok(n) => {
                        buf_end += n;
                    }
                    Err(err) => match err.kind() {
                        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset | ErrorKind::BrokenPipe | ErrorKind::TimedOut => {
                            // Transition state from `Playing` to `Disconnecting`
                            info!("{} left the world, because of error {err}", name_component.name);
                            commands.entity(entity).despawn();
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

mod schedule {
    use bevy::ecs::schedule::ScheduleLabel;

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ServerTickLabel();
}