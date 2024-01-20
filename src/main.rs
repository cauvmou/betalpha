use std::net::TcpListener;
use std::time::Instant;
use bevy::prelude::{App, Resource, Schedule, Update};
use log::{info, Level};
use crate::entity::{ClientStream, ConnectionState, PlayerBundle};
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


fn main() -> std::io::Result<()> {
    simple_logger::init_with_level(Level::Debug).expect("Failed to initialize logging!");
    let listener = TcpListener::bind("0.0.0.0:25565")?;
    listener.set_nonblocking(true)?;
    App::new()
        .add_schedule(Schedule::new(schedule::LoginLabel()))
        .add_schedule(Schedule::new(schedule::ProcessPacketsLabel()))
        .add_schedule(Schedule::new(schedule::ServerTickLabel()))
        .add_systems(Update, (
            system::accept_system,
            system::login_system,
            system::initializing_system
        ))
        .insert_resource(World::open("./ExampleWorld")?)
        .insert_resource(TcpWrapper { listener })
        .set_runner(|mut app: App| {
            let mut instant = Instant::now();
            loop {
                app.update();
                if instant.elapsed().as_millis() >= 50 {
                    // TODO: Tick
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
    use std::io::{BufReader, Cursor, Read, Write};
    use std::net::TcpStream;
    use bevy::prelude::{Commands, Entity, EventReader, EventWriter, Query, Res, ResMut, With};
    use bytes::{Buf, BytesMut};
    use log::{debug, error, info, warn};
    use crate::{BUFFER_SIZE, packet, TcpWrapper};
    use crate::byte_man::{get_string, get_u8};
    use crate::entity::{ClientStream, Named, PlayerBundle, PlayerChunkDB};
    use crate::entity::connection_state;
    use crate::event::{IncomingConnectionEvent};
    use crate::packet::{ids, PacketError, to_client_packets, to_server_packets};
    use crate::packet::{Deserialize, Serialize};
    use crate::world::World;

    pub fn accept_system(
        wrapper: Res<TcpWrapper>,
        mut commands: Commands,
    ) {
        if let Ok((mut stream, addr)) = wrapper.listener.accept() {
            info!("Got new connection {}", stream.peer_addr().unwrap());
            stream.set_nonblocking(false).unwrap();
            commands.spawn(ClientStream::<connection_state::Login>::new(stream));
        }
    }

    pub fn login_system(
        world: Res<World>,
        mut query: Query<(Entity, &ClientStream<connection_state::Login>), With<ClientStream<connection_state::Login>>>,
        mut commands: Commands,
    ) {
        #[derive(PartialEq)]
        enum InternalState {
            LoggingIn,
            LoggedIn,
        }
        for (entity, stream) in &mut query {
            {
                let mut stream = stream.stream.write().unwrap();
                let mut buf = [0u8; BUFFER_SIZE];
                let mut buf_position = 0usize;
                let mut state = InternalState::LoggingIn;
                pollster::block_on(async {
                    loop {
                        fn handle_packets<'w, 's>(stream: &mut TcpStream, buf: &[u8], entity: Entity, world: &World, commands: &mut Commands<'w, 's>, state: &mut InternalState) -> Result<usize, PacketError> {
                            let mut cursor = Cursor::new(&buf[..]);
                            while let Ok(packet_id) = get_u8(&mut cursor) {
                                match packet_id {
                                    ids::HANDSHAKE => {
                                        let name = get_string(&mut cursor)?;
                                        debug!("Received handshake with name {name:?}");
                                        let _ = stream.write(&[0x02, 0x00, 0x01, b'-']).unwrap();
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
                                        let _ = stream.write(&response.serialize()?).unwrap();
                                        stream.flush().unwrap();
                                        info!("Player \"{}\" joined the server!", request.username);
                                        *state = InternalState::LoggedIn;
                                    }
                                    _ => {
                                        return Err(PacketError::InvalidPacketID(packet_id));
                                    }
                                }
                            }
                            Ok(cursor.position() as usize)
                        }

                        if let Ok(n) = handle_packets(&mut stream, &buf[buf_position..], entity, &world, &mut commands, &mut state) {
                            debug!("Advancing buffer {n} bytes...");
                            buf_position += n;
                        } else {
                            debug!("Retrying to handle packets...")
                        }

                        if stream.read(&mut buf).unwrap() == 0 {
                            debug!("Read zero bytes...");
                            break;
                        }

                        if state == InternalState::LoggedIn {
                            break;
                        }
                    }
                });
            }
            commands.entity(entity).remove::<ClientStream<connection_state::Login>>().insert(ClientStream::<connection_state::Initializing>::from(stream.stream.clone()));
        }
    }

    pub fn initializing_system(
        mut world: ResMut<World>,
        mut query: Query<(Entity, &ClientStream<connection_state::Initializing>, &Named), (With<ClientStream<connection_state::Initializing>>, With<Named>)>,
        mut commands: Commands,
    ) {
        for (entity, stream, name_component) in &mut query {
            {
                let mut stream = stream.stream.write().unwrap();

                // Send chunk data
                let (player_chunk_x, player_chunk_z) = ((world.get_spawn()[0] - world.get_spawn()[0] % 16) / 16, (world.get_spawn()[2] - world.get_spawn()[2] % 16) / 16);
                debug!("Player {} is in spawned in chunk: [{player_chunk_x}, {player_chunk_z}].", name_component.name);
                let mut local_db = HashMap::with_capacity(8*8);
                for x in (player_chunk_x-4)..(player_chunk_x+4) {
                    for z in (player_chunk_z-4)..(player_chunk_z+4) {
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
                                stream.flush().unwrap();
                                let key = (x as u64) << 4 | z as u64;
                                local_db.insert(key, chunk);
                            }
                            Err(err) => {
                                error!("Failed to load chunk at (x: {x}, z: {z}): {err}!")
                            }
                        }
                    }
                }
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
                // TODO: Send position and look information
            }
            commands.entity(entity).remove::<ClientStream<connection_state::Initializing>>().insert(ClientStream::<connection_state::Playing>::from(stream.stream.clone()));
        }
    }

    //pub fn parse_packet_system<T>(
    //    mut ev_emmit_packet: EventWriter<RecvPacketEvent<T>>,
    //    mut query: Query<(Entity, &ClientStream<connection_state::Playing>), With<ClientStream<connection_state::Playing>>>,
    //) where T: Serialize + Deserialize + Send + Sync + 'static {
    //    for (entity, stream) in &mut query {
    //        let mut stream = stream.stream.write().unwrap();
    //        let mut buf = [0u8; BUFFER_SIZE];
    //        let mut buf_position = 0usize;
    //        pollster::block_on(async {
    //            loop {
    //                // TODO: Jesus...
    //                fn handle_packets<'w, 's, T>(stream: &mut TcpStream, buf: &[u8], entity: Entity, ev_emmit_packet: &mut EventWriter<RecvPacketEvent<T>>) -> Result<usize, PacketError> where T: Serialize + Deserialize + Send + Sync + 'static {
    //                    let mut cursor = Cursor::new(&buf[..]);
    //                    if let Ok(packet_id) = get_u8(&mut cursor) {
    //                        match packet_id {
    //                            ids::CHAT_MESSAGE => {
    //                                let packet to_server_packets::ChatMessagePacket::nested_deserialize(&mut cursor)?;
    //                                ev_emmit_packet.send(RecvPacketEvent { data: Box::new(packet) })
    //                            }
    //                            _ => {
    //                                error!("Client send invalid packet ID: {packet_id}");
    //                                return Err(PacketError::InvalidPacketID(packet_id));
    //                            }
    //                        }
    //                        Ok(cursor.position() as usize)
    //                    } else {
    //                        Err(PacketError::NotEnoughBytes)
    //                    }
//
    //                }
//
    //                if let Ok(n) = handle_packets(&mut stream, &buf[buf_position..], entity) {
    //                    debug!("Advancing buffer {n} bytes...");
    //                    buf_position += n;
    //                } else {
    //                    debug!("Retrying to handle packets...")
    //                }
//
    //                if stream.read(&mut buf).unwrap() == 0 {
    //                    debug!("Read zero bytes...");
    //                    break;
    //                }
    //            }
    //        });
    //    }
    //}

    pub fn tick_system(
        mut query: Query<(Entity, &ClientStream<connection_state::Playing>), With<ClientStream<connection_state::Playing>>>
    ) {
        for (entity, stream) in &mut query {}
    }
}

mod schedule {
    use bevy::ecs::schedule::ScheduleLabel;

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct LoginLabel();

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ProcessPacketsLabel();

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ServerTickLabel();
}