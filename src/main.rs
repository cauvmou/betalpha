use std::{
    collections::HashSet,
    future::Future,
    io::Cursor,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicI32, Ordering},
        Arc,
    },
};

use bytes::{Buf, BytesMut};

use nbt::{Blob, Value};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{
        broadcast::{self, error::TryRecvError},
        mpsc::{self, Sender},
        RwLock,
    },
};

// mod packet;

// use crate::packet::PacketError;
// use crate::packet::{util::*, Deserialize, Serialize};
use crate::util::base36_to_base10;
use crate::world::Chunk;

// mod byte_man;
// pub use byte_man::*;

// mod entities;
mod util;
mod world;

#[tokio::main]
async fn main() {

}