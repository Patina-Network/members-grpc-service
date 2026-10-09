use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    tonic_prost_build::configure()
        .file_descriptor_set_path(out_dir.join("descriptor.bin"))
        .compile_protos(
            &[
                "proto/v1/helloworld.proto",
                "proto/v1/members.proto",
                "proto/v1/core_admin.proto",
            ],
            &["proto/"],
        )?;

    Ok(())
}
