//! Add a unique compile-cache identity without changing BS-RoFormer weights or operators.
use crate::{Error, Result};
use onnx_protobuf::{Message, ModelProto, StringStringEntryProto};
use std::path::Path;
pub(super) const CACHE_VERSION: &str = "bs-roformer-host-stft-v1";
pub(super) fn separator(path: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    let mut model = ModelProto::parse_from_bytes(&bytes)
        .map_err(|e| Error::Model(format!("Read CoreML source graph: {e}")))?;
    model.metadata_props.retain(|p| p.key != "CACHE_KEY");
    model.metadata_props.push(StringStringEntryProto {
        key: "CACHE_KEY".into(),
        value: blake3::hash(format!("{}:{CACHE_VERSION}", blake3::hash(&bytes)).as_bytes())
            .to_hex()
            .to_string(),
        ..Default::default()
    });
    model
        .write_to_bytes()
        .map_err(|e| Error::Model(format!("Prepare CoreML graph: {e}")))
}
