use crate::testing::types::{TestRunRequest, TestSelection};

pub fn looks_like_test_request(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    parse_test_request(trimmed).is_some()
        || starts_with_any(
            trimmed,
            &[
                "测试",
                "验证",
                "测一下",
                "跑一轮",
                "回归",
                "重测",
                "只测",
            ],
        )
        || trimmed.eq_ignore_ascii_case("smoke test")
        || trimmed.eq_ignore_ascii_case("rerun failed")
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
            8,
        ));
    }

    if contains_any(trimmed, &["测试全部功能", "回归全部功能", "跑一轮全量回归", "跑一轮全部功能测试"]) {
        return Some(request(
            "全部功能回归",
            TestSelection {
                suite: None,
                feature: None,
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            true,
            32,
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
            8,
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
            8,
        ));
    }

    if contains_any(trimmed, &["回归浏览器适配器", "测试浏览器适配器", "测试浏览器功能"]) {
        return Some(request(
            "浏览器适配器回归",
            TestSelection {
                suite: Some("browser.adapter".to_string()),
                feature: None,
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            true,
            16,
        ));
    }

    if contains_any(trimmed, &["测试记事本输入", "测试记事本适配器", "回归记事本适配器"]) {
        return Some(request(
            "记事本适配器回归",
            TestSelection {
                suite: Some("notepad.adapter".to_string()),
                feature: None,
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            true,
            12,
        ));
    }

    if contains_any(trimmed, &["测试剪贴板读取", "测试剪贴板", "回归剪贴板"]) {
        return Some(request(
            "剪贴板能力回归",
            TestSelection {
                suite: Some("clipboard.core".to_string()),
                feature: None,
                tag: None,
                case_ids: vec![],
                rerun_failed_only: false,
            },
            false,
            8,
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
            32,
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
            16,
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
            8,
        ));
    }

    None
}

fn request(
    title: &str,
    selection: TestSelection,
    allow_supplementary_rerun: bool,
    max_cases: usize,
) -> TestRunRequest {
    TestRunRequest {
        title: title.to_string(),
        selection,
        dynamic_cases: vec![],
        max_cases,
        allow_supplementary_rerun,
    }
}

fn contains_any(input: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| input.contains(token))
}

fn starts_with_any(input: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| input.starts_with(prefix))
}
