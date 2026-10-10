//! Editor schema. Runtime validation additionally checks references, capabilities and Wasm resources.
use serde_json::{json, Value};
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
}
fn array(item: Value, max: usize) -> Value {
    json!({"type":"array","items":item,"maxItems":max})
}
fn integer(min: i64, max: i64) -> Value {
    json!({"type":"integer","minimum":min,"maximum":max})
}
fn text(max: usize) -> Value {
    json!({"type":"string","maxLength":max})
}
fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/$defs/{name}")})
}
fn nullable(v: Value) -> Value {
    json!({"anyOf":[v,{"type":"null"}]})
}

pub(super) fn preset_schema() -> Value {
    let target = reference("target");
    let number = json!({"type":"number"});
    let boolean = json!({"type":"boolean"});
    let bytes = array(integer(0, 255), 4096);
    let deck = json!({"enum":["a","b"]});
    let hid = object(
        json!({"protocol":{"const":"hid"},"vendor_id":integer(0,65535),"product_id":integer(0,65535),
        "interface":nullable(integer(-1,255)),"usage_page":nullable(integer(0,65535)),"usage":nullable(integer(0,65535))}),
        &["protocol", "vendor_id", "product_id"],
    );
    let midi = object(
        json!({"protocol":{"const":"midi"},"names":array(text(200),64)}),
        &["protocol"],
    );
    let endpoint = object(
        json!({"id":text(200),"matcher":{"oneOf":[hid,midi]}}),
        &["id", "matcher"],
    );
    let mut addresses = vec![];
    for (kind, fields) in [
        ("note", vec!["number"]),
        ("cc", vec!["number"]),
        ("cc14", vec!["msb", "lsb"]),
        ("pitch_bend", vec![]),
        ("rpn", vec!["parameter"]),
        ("nrpn", vec!["parameter"]),
    ] {
        let mut props = json!({"type":{"const":kind},"channel":integer(0,15)});
        let mut required = vec!["type", "channel"];
        for field in fields {
            props[field] = integer(0, if field == "parameter" { 16383 } else { 127 });
            required.push(field);
        }
        addresses.push(object(props, &required));
    }
    addresses.push(object(json!({"type":{"const":"sysex"},"prefix":bytes,"offset":integer(0,4095),"bytes":integer(1,4)}), &["type","prefix","offset","bytes"]));
    let source = json!({"oneOf":[
        object(json!({"protocol":{"const":"midi"},"address":{"oneOf":addresses}}), &["protocol","address"]),
        object(json!({"protocol":{"const":"hid"},"report_id":integer(0,255),"bit_offset":integer(8,32767),"bits":integer(1,32),"signed":boolean,"little_endian":boolean}), &["protocol","report_id","bit_offset","bits"])
    ]});
    let binding = object(
        json!({"id":text(200),"endpoint":text(200),"source":source,"target":target,
        "mode":{"enum":["auto","absolute","twos_complement","binary_offset","signed_bit","counter","button","momentary","toggle"]},
        "inverted":boolean,"pickup":boolean,"sensitivity":{"type":"number","exclusiveMinimum":0,"maximum":1000},
        "minimum":nullable(number.clone()),"maximum":nullable(number.clone()),"ticks_per_revolution":nullable(json!({"type":"number","exclusiveMinimum":0})),
        "layer":nullable(object(json!({"name":text(100),"active":boolean}), &["name","active"])),"sets_layer":nullable(text(100))}),
        &["id", "endpoint", "source", "target"],
    );
    let packet = object(
        json!({"endpoint":text(200),"data":bytes,"report":{"enum":["output","feature",null]}}),
        &["endpoint", "data"],
    );
    let feedback = object(
        json!({"signal":{"oneOf":[
        object(json!({"type":{"const":"control"},"target":target}), &["type","target"]),
        object(json!({"type":{"const":"meter"},"deck":nullable(deck)}), &["type"])
    ]},"packet":reference("packet"),"bit_offset":integer(8,32767),"bits":integer(1,32),"little_endian":boolean,"off":integer(0,u32::MAX as i64),"on":integer(0,u32::MAX as i64)}),
        &["signal", "packet", "bit_offset", "bits", "on"],
    );
    let evidence = object(
        json!({"model":text(200),"firmware":text(200),"os":text(200),"evidence_url":text(2048)}),
        &["model", "firmware", "os", "evidence_url"],
    );
    let community = object(
        json!({"package_id":{"type":"string","pattern":"^[a-z0-9._-]{1,100}$"},"package_version":text(100),"minimum_host_version":text(100),
        "author":text(200),"license":text(200),"license_text":text(65536),"source_url":text(2048),"tested_on":array(evidence,32)}),
        &[
            "package_id",
            "package_version",
            "minimum_host_version",
            "author",
            "license",
            "license_text",
            "source_url",
        ],
    );
    let permission = object(
        json!({"endpoint":text(200),"access":{"enum":["output","feature_write","feature_read"]},"report_id":integer(0,255),"length":integer(1,1024)}),
        &["endpoint", "access", "report_id", "length"],
    );
    let adapter = object(
        json!({"api_version":{"const":1},"wasm_base64":{"type":"string","contentEncoding":"base64","maxLength":349528},"blake3":{"type":"string","pattern":"^[a-f0-9]{64}$"},"controls":array(target,256),"reports":array(permission,128)}),
        &["api_version", "wasm_base64", "blake3", "controls"],
    );
    let provenance = object(
        json!({"url":text(2048),"revision":text(200),"section":text(4096)}),
        &["url", "revision", "section"],
    );
    let pair = json!({"type":"array","items":integer(0,65535),"minItems":2,"maxItems":2});
    let mut schema = object(
        json!({"version":{"const":1},"id":text(200),"name":text(200),"manufacturer":text(200),"notes":text(4096),
        "provenance":array(provenance,64),"endpoints":array(endpoint,32),"bindings":array(binding,4096),"feedback":array(feedback,2048),
        "initialize":array(reference("packet"),128),"shutdown":array(reference("packet"),128),
        "audio":nullable(object(json!({"device_name":text(200),"master":pair,"cue":pair}), &["device_name","master","cue"])),
        "verification":{"enum":["protocol","simulation","hardware"]},"community":nullable(community),"adapter":nullable(adapter)}),
        &["version", "id", "name"],
    );
    schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    schema["title"] = json!("mixless controller preset 1 / adapter API 1");
    schema["description"] = json!("Editor assistance. Run mixless-controller validate for semantic references, compatibility, checksums and sandbox limits.");
    schema["$defs"] =
        json!({"packet":packet,"target":{"enum":mixless_controllers::MidiTarget::all()}});
    schema
}

#[cfg(test)]
mod tests {
    #[test]
    fn published_schema_matches_generator() {
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../../../sdk/controller-preset.schema.json"))
                .unwrap();
        assert_eq!(super::preset_schema(), expected);
    }
}
