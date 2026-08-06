#[path = "../src/comfy.rs"]
mod comfy;
#[path = "../src/workflow_registry.rs"]
mod workflow_registry;

use comfy::ProbeResult;
use std::collections::BTreeSet;
use workflow_registry::{WorkflowMode, WorkflowProvenance, registered_workflows, select_workflow};

#[test]
fn registers_t2v_and_ref2va_with_explicit_fixture_provenance() {
    let workflows = registered_workflows();
    assert_eq!(workflows.len(), 2);
    assert!(workflows.iter().all(|workflow| {
        workflow.version == 1
            && workflow.provenance == WorkflowProvenance::ProjectFixture
            && workflow.source_url.is_none()
            && workflow.title.contains("开发夹具")
    }));
}

#[test]
fn selects_and_loads_each_mode() {
    let t2v = select_workflow(WorkflowMode::T2v).unwrap();
    assert_eq!(t2v.id, "h3-t2v-fixture");
    assert_eq!(t2v.load().unwrap().id, t2v.id);

    let ref2va = select_workflow(WorkflowMode::Ref2va).unwrap();
    assert_eq!(ref2va.id, "h3-ref2va-fixture");
    let template = ref2va.load().unwrap();
    assert_eq!(template.id, ref2va.id);
    assert!(template.required_node_types.contains(&"LoadAudio".into()));
}

#[test]
fn bundled_bytes_match_registered_sha256() {
    for workflow in registered_workflows() {
        assert_eq!(workflow.actual_sha256(), workflow.sha256, "{}", workflow.id);
        assert!(workflow.bundled_json().contains("NOT AN OFFICIAL"));
    }
}

#[test]
fn reports_missing_node_capabilities_for_selected_template() {
    let workflow = select_workflow(WorkflowMode::T2v).unwrap();
    let report = workflow
        .capability_report(&ProbeResult {
            reachable: true,
            node_types: BTreeSet::from([
                "H3ModelLoader".into(),
                "H3TextEncode".into(),
                "VHS_VideoCombine".into(),
            ]),
        })
        .unwrap();
    assert!(!report.compatible);
    assert_eq!(report.missing_node_types, vec!["H3Sampler"]);
}
