use crate::testing::types::{TestRunRequest, TestSelection};

pub fn looks_like_test_request(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    ["测试", "测一下", "smoke", "回归", "重测", "failed", "安全相关"]
        .iter()
        .any(|token| trimmed.contains(token))
}

pub fn parse_test_request(input: &str) -> Option<TestRunRequest> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if contains_any(trimmed, &["测一下浏览器地址栏", "测试浏览器地址栏"]) {
        return Some(request(
            "浏览器地址栏测试",
            TestSelection {
                suite: None,
                feature: Some("browser.address_bar".to_string()),
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            true,
        ));
    }

    if contains_any(trimmed, &["跑一轮 smoke test", "跑一轮 smoke", "smoke test"]) {
        return Some(request(
            "Smoke Test",
            TestSelection {
                suite: Some("smoke.desktop_agent".to_string()),
                feature: None,
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            true,
        ));
    }

    if contains_any(trimmed, &["回归视觉理解", "测试视觉理解", "视觉回归"]) {
        return Some(request(
            "视觉理解回归",
            TestSelection {
                suite: Some("vision.core".to_string()),
                feature: None,
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            true,
        ));
    }

    if contains_any(trimmed, &["重测上次失败项", "重跑上次失败项", "rerun failed"]) {
        return Some(request(
            "重测上次失败项",
            TestSelection {
                suite: None,
                feature: None,
                tag: None,
                case_ids: vec![],
                rerun_failed_only: true,
            },
            false,
        ));
    }

    if contains_any(trimmed, &["只测安全相关", "测试安全相关", "安全回归"]) {
        return Some(request(
            "安全相关测试",
            TestSelection {
                suite: None,
                feature: None,
                tag: Some("safety".to_string()),
                case_ids: vec![],
                rerun_failed_only: false,
            },
            false,
        ));
    }

    if contains_any(trimmed, &["测试微信草稿输入", "测一下微信草稿输入"]) {
        return Some(request(
            "微信草稿输入测试",
            TestSelection {
                suite: Some("wechat.draft".to_string()),
                feature: Some("wechat.draft_input".to_string()),
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            false,
        ));
    }

    None
}

fn request(title: &str, selection: TestSelection, allow_supplementary_rerun: bool) -> TestRunRequest {
    TestRunRequest {
        title: title.to_string(),
        selection,
        max_cases: 8,
        allow_supplementary_rerun,
    }
}

fn contains_any(input: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| input.contains(token))
}
