use bevy_ecs::schedule::Schedule;
use tokio::net::TcpListener;
use crate::entity::{ClientStream, ConnectionState, PlayerBundle};
use crate::entity::connection_state::Handshake;
use crate::world::World;

mod util;
mod world;
mod entity;
mod packet;

pub(crate) const BUFFER_SIZE: usize = 1024 * 8;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:25565").await.unwrap();

    let minecraft_world = World::open("./ExampleWorld")?;

    let mut ecs = bevy_ecs::world::World::new();
    ecs.add_schedule(Schedule::new(schedule::HandshakeLabel()));
    ecs.register_system(system::handshake_system);
    ecs.register_system(system::login_system);

    loop {
        if let Ok((stream, addr)) = listener.accept().await {
            ecs.spawn(PlayerBundle {
                stream: ClientStream::<Handshake>::new(stream),
                position: Default::default(),
                velocity: Default::default(),
                look: Default::default(),
                name: Default::default(),
            });
        }
    }
}

mod system {
    use std::io::Cursor;
    use bevy_ecs::prelude::{Commands, Entity, Query, With};
    use bytes::BytesMut;
    use tokio::io::AsyncReadExt;
    use crate::BUFFER_SIZE;
    use crate::entity::{ClientStream};
    use crate::entity::connection_state::{Handshake, Login};

    pub fn handshake_system(mut query: Query<(Entity, &ClientStream<Handshake>), With<ClientStream<Handshake>>>, mut commands: Commands) {
        for (entity, stream) in &mut query {
            {
                let mut stream = stream.stream.write().unwrap();
                let mut buf = BytesMut::with_capacity(BUFFER_SIZE);
                pollster::block_on(stream.read_buf(&mut buf)).unwrap();

            }
            commands.entity(entity).remove::<ClientStream<Handshake>>().insert(ClientStream::<Login>::from(stream.stream.clone()));
        }
    }

    pub fn login_system(mut query: Query<(Entity, &ClientStream<Login>), With<ClientStream<Login>>>, mut commands: Commands) {
        for (entity, stream) in &mut query {
            let stream = &stream.stream;
            commands.entity(entity).remove::<ClientStream<Handshake>>().insert(ClientStream::<Login>::from(stream.clone()));
        }
    }
}

mod schedule {
    use bevy_ecs::schedule::ScheduleLabel;

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct HandshakeLabel();
}