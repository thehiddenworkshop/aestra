use super::*;
use serde_json::{Value, json};

fn cube() -> Value {
    serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/test/meshes/lab_cube.gltf"
    )))
    .unwrap()
}
fn blob(document: &Value) -> Vec<u8> {
    STANDARD
        .decode(
            document["buffers"][0]["uri"]
                .as_str()
                .unwrap()
                .split_once(',')
                .unwrap()
                .1,
        )
        .unwrap()
}
fn write(root: &Path, name: &str, document: &Value) {
    fs::write(root.join(name), serde_json::to_vec(document).unwrap()).unwrap();
}
fn preview(root: &Path, name: &str) -> Result<Vec<u8>, String> {
    render(root, Path::new(name), &AtomicBool::new(false))
}

#[test]
fn renderer_primitive_reads_fresh_geometry_and_preserves_optional_inputs() {
    let root = tempfile::tempdir().unwrap();
    let mut document = cube();
    write(root.path(), "cube.gltf", &document);
    let reference = "cube.gltf#Mesh0/Primitive0";
    let flag = AtomicBool::new(false);
    let (original, radius) = load_primitive(root.path(), reference, &flag).unwrap();
    assert_eq!(original.count_vertices(), 36);
    assert!(original.contains_attribute(Mesh::ATTRIBUTE_UV_1));
    assert!(original.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
    let mut data = blob(&document);
    let stride = document["bufferViews"][0]["byteStride"].as_u64().unwrap() as usize;
    for vertex in data.chunks_exact_mut(stride) {
        for component in vertex[..12].as_chunks_mut::<4>().0 {
            *component = (f32::from_le_bytes(*component) * 2.0).to_le_bytes();
        }
    }
    document["buffers"][0]["uri"] = json!(format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(data)
    ));
    document["meshes"][0]["primitives"][0]["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("TEXCOORD_1");
    document["meshes"][0]["primitives"][0]["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("TANGENT");
    write(root.path(), "cube.gltf", &document);
    let (changed, changed_radius) = load_primitive(root.path(), reference, &flag).unwrap();
    assert_eq!(changed_radius, radius * 2.0);
    assert!(!changed.contains_attribute(Mesh::ATTRIBUTE_UV_1));
    assert!(!changed.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
    assert!(load_primitive(root.path(), reference, &AtomicBool::new(true)).is_err());
}

#[test]
fn embedded_external_and_binary_meshes_render_the_same_static_geometry() {
    let root = tempfile::tempdir().unwrap();
    let mut document = cube();
    write(root.path(), "cube.gltf", &document);
    let original = fs::read(root.path().join("cube.gltf")).unwrap();
    let expected = preview(root.path(), "cube.gltf").unwrap();
    assert_eq!(expected.len(), (EDGE * EDGE * 4) as usize);
    let filled = expected
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| **pixel != BACKGROUND)
        .count();
    assert!(filled > 100 && filled < (EDGE * EDGE) as usize * 9 / 10);
    assert_eq!(&expected[..4], BACKGROUND);
    assert_eq!(fs::read(root.path().join("cube.gltf")).unwrap(), original);
    let mut binary = blob(&document);
    document["buffers"][0]
        .as_object_mut()
        .unwrap()
        .remove("uri");
    let mut json = serde_json::to_vec(&document).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let mut glb = b"glTF".to_vec();
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&((28 + json.len() + binary.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&binary);
    fs::write(root.path().join("cube.glb"), glb).unwrap();
    assert_eq!(preview(root.path(), "cube.glb").unwrap(), expected);
    fs::create_dir(root.path().join("meshes")).unwrap();
    document["buffers"][0]["uri"] = json!("../buffer%20data.bin");
    fs::write(root.path().join("buffer data.bin"), &binary).unwrap();
    write(root.path(), "meshes/cube.gltf", &document);
    assert_eq!(preview(root.path(), "meshes/cube.gltf").unwrap(), expected);
    assert_eq!(
        fs::read(root.path().join("buffer data.bin")).unwrap(),
        binary
    );
}

#[test]
fn mesh_scene_transforms_are_framed_and_multiple_instances_are_visible() {
    let root = tempfile::tempdir().unwrap();
    let mut document = cube();
    write(root.path(), "cube.gltf", &document);
    let original = preview(root.path(), "cube.gltf").unwrap();
    document["nodes"][0]["translation"] = json!([10.0, -8.0, 4.0]);
    document["nodes"][0]["scale"] = json!([2.0, 2.0, 2.0]);
    write(root.path(), "cube.gltf", &document);
    assert_eq!(preview(root.path(), "cube.gltf").unwrap(), original);
    document["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"mesh":0,"translation":[15.0,-8.0,4.0]}));
    document["scenes"][0]["nodes"] = json!([0, 1]);
    write(root.path(), "cube.gltf", &document);
    assert_ne!(preview(root.path(), "cube.gltf").unwrap(), original);
}

#[test]
fn indexed_meshes_match_unindexed_geometry_and_reject_invalid_indices() {
    let root = tempfile::tempdir().unwrap();
    let mut document = cube();
    write(root.path(), "cube.gltf", &document);
    let expected = preview(root.path(), "cube.gltf").unwrap();
    let mut bytes = blob(&document);
    let offset = bytes.len();
    let count = document["accessors"][0]["count"].as_u64().unwrap() as usize;
    for index in 0..count {
        bytes.extend_from_slice(&(index as u16).to_le_bytes());
    }
    let view = document["bufferViews"].as_array().unwrap().len();
    document["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":offset,"byteLength":count*2}));
    let accessor = document["accessors"].as_array().unwrap().len();
    document["accessors"]
        .as_array_mut()
        .unwrap()
        .push(json!({"bufferView":view,"componentType":5123,"count":count,"type":"SCALAR"}));
    document["meshes"][0]["primitives"][0]["indices"] = json!(accessor);
    document["buffers"][0]["byteLength"] = json!(bytes.len());
    document["buffers"][0]["uri"] = json!(format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(&bytes)
    ));
    write(root.path(), "cube.gltf", &document);
    assert_eq!(preview(root.path(), "cube.gltf").unwrap(), expected);
    bytes[offset..offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    document["buffers"][0]["uri"] = json!(format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(&bytes)
    ));
    write(root.path(), "cube.gltf", &document);
    assert!(
        preview(root.path(), "cube.gltf")
            .unwrap_err()
            .contains("index")
    );
}

#[test]
fn mesh_buffers_cannot_escape_root_or_request_network_resources() {
    for uri in [
        "../../outside.bin",
        "/absolute.bin",
        "C:/outside.bin",
        "https://host/data.bin",
        "../%2e%2e/outside",
        "%5c%5cserver%5cdata",
        "bad%xy",
    ] {
        assert!(
            buffer_path(Path::new("meshes/cube.gltf"), uri).is_err(),
            "{uri}"
        );
    }
    assert_eq!(
        buffer_path(Path::new("meshes/cube.gltf"), "../data.bin").unwrap(),
        PathBuf::from("data.bin")
    );
    let root = tempfile::tempdir().unwrap();
    let mut document = cube();
    document["buffers"][0]["uri"] = json!("missing.bin");
    write(root.path(), "cube.gltf", &document);
    assert!(preview(root.path(), "cube.gltf").is_err());
    fs::write(root.path().join("missing.bin"), b"short").unwrap();
    assert!(
        preview(root.path(), "cube.gltf")
            .unwrap_err()
            .contains("truncated")
    );
}

#[test]
fn malformed_large_cyclic_and_cancelled_meshes_fail_without_panics() {
    let root = tempfile::tempdir().unwrap();
    for mutate in [
        |doc: &mut Value| doc["accessors"][0]["count"] = json!(100001),
        |doc: &mut Value| doc["accessors"][0]["byteOffset"] = json!(999999),
        |doc: &mut Value| doc["nodes"][0]["children"] = json!([0]),
        |doc: &mut Value| doc["meshes"][0]["primitives"][0]["mode"] = json!(1),
        |doc: &mut Value| doc["buffers"][0]["byteLength"] = json!(FILE_LIMIT + 1),
    ] {
        let mut document = cube();
        mutate(&mut document);
        write(root.path(), "cube.gltf", &document);
        assert!(preview(root.path(), "cube.gltf").is_err());
    }
    let mut document = cube();
    let mut bytes = blob(&document);
    bytes[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    document["buffers"][0]["uri"] = json!(format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(bytes)
    ));
    write(root.path(), "cube.gltf", &document);
    assert!(
        preview(root.path(), "cube.gltf")
            .unwrap_err()
            .contains("Non-finite")
    );
    assert_eq!(
        render(root.path(), Path::new("cube.gltf"), &AtomicBool::new(true)).unwrap_err(),
        "Cancelled"
    );
    assert!(preview(root.path(), "unsupported.obj").is_err());
    assert!(rasterize(&[], &AtomicBool::new(false)).is_err());
    let triangle = [Vec3::ZERO, Vec3::X, Vec3::Y];
    assert!(
        rasterize(&vec![triangle; 2000], &AtomicBool::new(false))
            .unwrap_err()
            .contains("budget")
    );
}
