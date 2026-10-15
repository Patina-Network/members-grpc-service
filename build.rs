use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = "src/proto";
    std::fs::create_dir_all(out_dir)?;

    let out_dir = PathBuf::from(out_dir);

    tonic_prost_build::configure()
        .out_dir(&out_dir)
        .file_descriptor_set_path(out_dir.join("descriptor.bin"))
        // warning: large size difference between variants
        // src/proto/members.admin.rs:126:5
        // pub enum Outcome {
        // }
        // ^ the entire enum is at least 456 bytes
         .boxed(".members.admin.MemberOperationResult.outcome.updated")
        .compile_protos(
            &["proto/v1/members.proto", "proto/v1/members_admin.proto"],
            &["proto/"],
        )?;

    Ok(())
}
