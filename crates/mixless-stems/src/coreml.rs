//! Adapt only the pinned HTDemucs graph, without modifying its weights or source file.
use crate::{Error, Result};
use onnx_protobuf::{
    attribute_proto::AttributeType, AttributeProto, Message, ModelProto, StringStringEntryProto,
};
use std::path::Path;

// Bump when graph adaptation, ORT, format, or compute-unit policy changes.
pub(super) const CACHE_VERSION: &str = "ort-1.28-mlprogram-gpu-v1";

pub(super) fn separator(path: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    let mut model = ModelProto::parse_from_bytes(&bytes)
        .map_err(|e| Error::Model(format!("Read CoreML source graph: {e}")))?;
    adapt(&mut model)?;
    model.metadata_props.push(StringStringEntryProto {
        key: "CACHE_KEY".into(),
        value: blake3::hash(
            format!("{}:{CACHE_VERSION}", crate::models::SEPARATOR_HASH).as_bytes(),
        )
        .to_hex()
        .to_string(),
        ..Default::default()
    });
    model
        .write_to_bytes()
        .map_err(|e| Error::Model(format!("Prepare CoreML graph: {e}")))
}

fn adapt(model: &mut ModelProto) -> Result<()> {
    let graph = model
        .graph
        .as_mut()
        .ok_or_else(|| Error::Model("Missing separator graph".into()))?;
    // These two 4096-tap convolutions implement iSTFT, not learned layers.
    // CoreML's implementation takes ~24 s/chunk on M5 Max. Explicitly declaring
    // their existing output length preserves ONNX semantics and makes ORT 1.28
    // leave them on CPU (CoreML EP does not support ConvTranspose.output_shape).
    // Keep the remainder of the network eligible for GPU execution.
    for name in ["/real_istft/ConvTranspose", "/real_istft/ConvTranspose_1"] {
        let node = graph
            .node
            .iter_mut()
            .find(|n| n.name == name)
            .ok_or_else(|| Error::Model(format!("Missing pinned separator node: {name}")))?;
        let ints = |key: &str| {
            node.attribute
                .iter()
                .find(|a| a.name == key)
                .map(|a| a.ints.as_slice())
        };
        if node.op_type != "ConvTranspose"
            || ints("kernel_shape") != Some(&[4096][..])
            || ints("strides") != Some(&[1024][..])
            || ints("dilations") != Some(&[1][..])
            || ints("pads") != Some(&[0, 0][..])
            || node.attribute.iter().any(|a| a.name == "output_shape")
        {
            return Err(Error::Model(format!(
                "Unexpected pinned separator node: {name}"
            )));
        }
        // (340 spectral frames - 1) * 1024 hop + 4096 window = 351232.
        node.attribute.push(AttributeProto {
            name: "output_shape".into(),
            type_: AttributeType::INTS.into(),
            ints: vec![351232],
            ..Default::default()
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use onnx_protobuf::{GraphProto, NodeProto};

    #[test]
    fn adaptation_only_adds_output_shapes_and_rejects_unknown_graphs() {
        let mut model = ModelProto::new();
        assert!(adapt(&mut model).is_err());
        let mut graph = GraphProto::new();
        for name in ["/real_istft/ConvTranspose", "/real_istft/ConvTranspose_1"] {
            let mut node = NodeProto {
                name: name.into(),
                op_type: "ConvTranspose".into(),
                ..Default::default()
            };
            for (name, ints) in [
                ("kernel_shape", vec![4096]),
                ("strides", vec![1024]),
                ("dilations", vec![1]),
                ("pads", vec![0, 0]),
            ] {
                node.attribute.push(AttributeProto {
                    name: name.into(),
                    ints,
                    type_: AttributeType::INTS.into(),
                    ..Default::default()
                });
            }
            graph.node.push(node);
        }
        model.graph = Some(graph).into();
        let before = model.clone();
        adapt(&mut model).unwrap();
        for node in &mut model.graph.as_mut().unwrap().node {
            let shape = node.attribute.pop().unwrap();
            assert_eq!(shape.name, "output_shape");
            assert_eq!(shape.ints, [351232]);
        }
        assert_eq!(model, before);
        model.graph.as_mut().unwrap().node[0].attribute[0].ints[0] = 2048;
        assert!(adapt(&mut model).is_err());
    }
}
