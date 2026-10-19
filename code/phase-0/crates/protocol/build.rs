fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: this build script sets PROTOC before invoking the local code generator.
    unsafe { std::env::set_var("PROTOC", protoc) };
    tonic_prost_build::compile_protos("../../proto/cluster.proto")?;
    Ok(())
}
