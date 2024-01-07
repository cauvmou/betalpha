pub enum ToClientPacket {
    KeepAlive,
    LoginResponse {
        entity_id: u32,
        _unused1: String,
        _unused2: String,
        map_seed: u64,
        dimension: u8,
    },
    Handshake {
        connection_hash: String,
    },
    ChatMessage {
        message: String,
    },
    TimeUpdate {
        time: u64,
    },
    PlayerInventory {
        inventory_type: i32,
        count: u16,
        payload: Vec<u8>,
    },
    SpawnPosition {
        x: i32,
        y: i32,
        z: i32,
    },
    UpdateHealth {
        health: u8,
    },
    Respawn,
    PlayerPositionLook {
        x: f64,
        stance: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    },
    HoldingChange {
        entity_id: u32,
        item_id: u16,
    },
    AddToInventory {
        item_type: u16,
        count: u8,
        life: u16,
    },
    Animation {
        entity_id: u32,
        animate: u8,
    },
    NamedEntitySpawn {
        entity_id: u32,
        name: String,
        x: i32,
        y: i32,
        z: i32,
        rotation: i8,
        pitch: i8,
        current_item: u16,
    },
    PickupSpawn {
        entity_id: u32,
        item_id: u16,
        count: u8,
        x: i32,
        y: i32,
        z: i32,
        rotation: u8,
        pitch: i8,
        roll: i8,
    },
    CollectItem {
        collected_entity_id: u32,
        collector_entity_id: u32,
    },
    AddObjectOrVehicle {
        entity_id: u32,
        object_type: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    MobSpawn {
        entity_id: u32,
        mob_type: u8,
        x: i32,
        y: i32,
        z: i32,
        yaw: i8,
        pitch: i8,
    },
    EntityVelocity {
        entity_id: u32,
        vel_x: i16,
        vel_y: i16,
        vel_z: i16,
    },
    DestroyEntity {
        entity_id: u32,
    },
    Entity {
        entity_id: u32
    },
    EntityRelativeMove {
        entity_id: u32,
        x: i8,
        y: i8,
        z: i8,
    },
    EntityLook {
        entity_id: u32,
        yaw: i8,
        pitch: i8,
    },
    EntityLookRelativeMove {
        entity_id: u32,
        x: i8,
        y: i8,
        z: i8,
        yaw: i8,
        pitch: i8,
    },
    EntityTeleport {
        entity_id: u32,
        x: i32,
        y: i32,
        z: i32,
        yaw: i8,
        pitch: i8,
    },
    EntityStatus {
        entity_id: u32,
        entity_status: u8,
    },
    AttachEntity {
        entity_id: u32,
        vehicle_id: u32,
    },
    PreChunk {
        x: i32,
        z: i32,
        mode: bool,
    },
    MapChunk {
        x: i32,
        y: i16,
        z: i32,
        size_x: i8,
        size_y: i8,
        size_z: i8,
        compressed_size: i32,
        compressed_data: Vec<u8>,
    },
    MultiBlockChange {
        chunk_x: i32,
        chunk_y: i32,
        array_size: u16,
        coordinate_array: Vec<i16>,
        type_array: Vec<u8>,
        metadata_array: Vec<u8>,
    },
    BlockChange {
        x: i32,
        y: i8,
        z: i32,
        block_type: u8,
        block_metadata: u8,
    },
    ComplexEntities {
        x: i32,
        y: i16,
        z: i32,
        payload_size: u16,
        payload: Vec<u8>,
    },
    Explosion {
        x: f64,
        y: f64,
        z: f64,
        radius: f32,
        record_count: u32,
        records: Vec<u8>,
    },
    Kick {
        reason: String,
    },
}

pub enum ToServerPacket {
    KeepAlive,
    LoginResponse {
        entity_id: u32,
        _unused1: String,
        _unused2: String,
        map_seed: u64,
        dimension: u8,
    },
    Handshake {
        connection_hash: String,
    },
    ChatMessage {
        message: String,
    },
    PlayerInventory {
        inventory_type: i32,
        count: u16,
        payload: Vec<u8>,
    },
    UseEntity {
        entity_id: u32,
        target_id: u32,
        is_left_click: bool,
    },
    Respawn,
    Player {
        on_ground: bool,
    },
    PlayerPosition {
        x: f64,
        y: f64,
        stance: f64,
        z: f64,
        on_ground: bool,
    },
    PlayerLook {
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    },
    PlayerPositionLook {
        x: f64,
        y: f64,
        stance: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    },
    PlayerDigging {
        status: u8,
        x: i32,
        y: i8,
        z: i32,
        face: u8,
    },
    PlayerBlockPlacement {
        item_id: u16,
        x: i32,
        y: i8,
        z: i32,
        face: u8,
    },
    HoldingChange {
        _unused: i32,
        item_id: u16,
    },
    ArmAnimation {
        entity_id: u32,
        animate: bool,
    },
    PickupSpawn {
        entity_id: u32,
        item_id: u16,
        count: u8,
        x: i32,
        y: i32,
        z: i32,
        rotation: i8,
        pitch: i8,
        roll: i8,
    },
    Disconnect {
        reason: String
    },
}