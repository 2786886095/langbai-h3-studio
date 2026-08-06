#[path = "../src/model_store.rs"]
mod model_store;

use model_store::{H3ModelType, ModelIntegrity, scan_model_directory};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("langbai-model-scan-{id}"));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn finds_complete_fl2va_and_reports_sizes() {
    let root = fixture();
    let model = root.join("MiniMax-H3-FL2VA");
    fs::create_dir_all(&model).unwrap();
    fs::write(model.join("model_index.json"), br#"{"model_type":"fl2va"}"#).unwrap();
    fs::write(model.join("weights.safetensors"), [1_u8, 2, 3, 4]).unwrap();

    let result = scan_model_directory(&root, None).unwrap();
    assert_eq!(result.models.len(), 1);
    assert_eq!(result.models[0].model_type, H3ModelType::Fl2Va);
    assert_eq!(result.models[0].integrity, ModelIntegrity::Complete);
    assert_eq!(result.models[0].safetensors_count, 1);
    assert!(result.models[0].total_size_bytes >= 4);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recognizes_ref2va_from_index_and_flags_missing_weights() {
    let root = fixture();
    let model = root.join("downloaded-model");
    fs::create_dir_all(&model).unwrap();
    fs::write(model.join("model_index.json"), br#"{"pipeline":"Ref2VA"}"#).unwrap();

    let result = scan_model_directory(&root, Some(2)).unwrap();
    assert_eq!(result.models[0].model_type, H3ModelType::Ref2Va);
    assert_eq!(result.models[0].integrity, ModelIntegrity::Partial);
    assert!(
        result.models[0]
            .warnings
            .iter()
            .any(|w| w.contains("safetensors"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn obeys_depth_limit_and_marks_zero_length_weights_invalid() {
    let root = fixture();
    let shallow = root.join("a");
    let deep = shallow.join("b");
    fs::create_dir_all(&deep).unwrap();
    fs::write(shallow.join("FL2VA.safetensors"), []).unwrap();
    fs::write(deep.join("Ref2VA.safetensors"), [7_u8]).unwrap();

    let result = scan_model_directory(&root, Some(1)).unwrap();
    assert_eq!(result.models.len(), 1);
    assert_eq!(result.models[0].integrity, ModelIntegrity::Invalid);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_non_directory_roots() {
    let root = fixture();
    let file = root.join("model.safetensors");
    fs::write(&file, [1_u8]).unwrap();
    assert!(scan_model_directory(&file, None).is_err());
    fs::remove_dir_all(root).unwrap();
}
