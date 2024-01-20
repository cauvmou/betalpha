use std::marker::PhantomData;
use bevy::prelude::Event;
use crate::packet::{Deserialize, Serialize};

#[derive(Event)]
pub struct IncomingConnectionEvent {}