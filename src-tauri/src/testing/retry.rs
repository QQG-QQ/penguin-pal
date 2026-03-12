use serde_json::json;

use super::types::{FailureItem, TestCase, TestStep};

pub fn should_rerun_failure(case: &TestCase, failure: &FailureItem) -> bool {
    if case.max_probes == 0 {
        return false;
    }

    match failure.failure_stage {
        super::types::TestFailureStage::StepExecute
        | super::types::TestFailureStage::Assertion
        | super::types::TestFailureStage::Probe => true,
        _ => false,
    }
}

pub fn supplementary_probes(case: &TestCase, failure: &FailureItem) -> Vec<TestStep> {
    let mut probes = Vec::new();

    if matches!(failure.failure_stage, super::types::TestFailureStage::StepExecute)
        && matches!(
            case.test_target_policy,
            super::types::TestTargetPolicy::NamedWindowRequired
        )
    {
        probes.push(TestStep::ControlInvoke {
            tool: "list_windows".to_string(),
            args: json!({}),
            summary: "补测：重新列出窗口".to_string(),
        });
    }

    probes.push(TestStep::CaptureScreenContext {
        summary: "补测：重新采集屏幕上下文".to_string(),
    });

    probes.truncate(case.max_probes);
    probes
}
