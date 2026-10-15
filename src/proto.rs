pub mod members {
    include!("proto/members.rs");

    pub mod admin {
        include!("proto/members.admin.rs");
    }
}

pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!("proto/descriptor.bin");
