fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::compile_protos(&["proto/onnx.proto3"], &["proto/"])?;
    Ok(())
}
