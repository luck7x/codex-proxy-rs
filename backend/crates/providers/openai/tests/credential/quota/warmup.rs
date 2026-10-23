//! 验证额度预热按 SSE 终态判定结果、请求档位、单号时刻与窗口占位，并跳过过期凭据

use std::time::UNIX_EPOCH;

use gateway_core::account::{OpaqueProviderData, QuotaObservation};

use super::*;

async fn due_account_ids(
    service: &CodexCredentialQuotaService,
    hour: u32,
    minute: u32,
    global_due: bool,
) -> Vec<String> {
    let mut ids = service
        .warmup_due_accounts(hour, minute, global_due)
        .await
        .expect("due accounts")
        .iter()
        .map(|account| account.id().to_string())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

async fn seed_short_term_window(
    store: &Arc<MemoryAccountStore>,
    id: &str,
    observed_at: SystemTime,
    reset_at: u64,
) {
    create_account(store, id).await;
    let account = store.account(id).expect("created account");
    store
        .compare_and_swap_quota(QuotaObservation {
            plan_type: None,
            account_id: account.id().clone(),
            expected_revision: account.revision(),
            quota: OpaqueProviderData::new(
                json!({"rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": 0,
                        "reset_at": reset_at,
                        "limit_window_seconds": 18_000
                    }
                }})
                .as_object()
                .expect("quota object")
                .clone(),
            ),
            observed_at,
            state: QuotaState::allowed(observed_at),
        })
        .await
        .expect("persist quota");
}

#[tokio::test]
async fn warmup_consumes_completed_sse_and_persists_rate_limit_event() {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: codex.rate_limits\n",
        "data: {\"type\":\"codex.rate_limits\",\"plan_type\":\"team\",\"rate_limits\":{\"allowed\":true,\"limit_reached\":false,\"primary\":{\"used_percent\":5,\"window_minutes\":300,\"reset_at\":1900000000}}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_warmup\",\"status\":\"completed\",\"output\":[]}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let store = Arc::new(MemoryAccountStore::default());
    create_account(&store, "acct_warmup_completed").await;
    let service = quota_service_with_base_url(
        &store,
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        server.uri(),
    );

    let accounts = service
        .warmup_due_accounts(8, 0, true)
        .await
        .expect("due accounts");
    let summary = service
        .execute_warmup("test-model", accounts)
        .await
        .expect("warmup");

    assert_eq!(summary.warmed_up, 1);
    assert_eq!(summary.failed, 0);
    assert_eq!(
        store.account("acct_warmup_completed").unwrap().plan_type(),
        Some("team")
    );
    assert!(store.quota_json("acct_warmup_completed").is_some());
}

#[tokio::test]
async fn warmup_sends_the_lowest_reasoning_effort_supported_by_the_catalog() {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_warmup_effort\",\"status\":\"completed\",\"output\":[]}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let store = Arc::new(MemoryAccountStore::default());
    create_account(&store, "acct_warmup_effort").await;
    let service = quota_service_with_base_url(
        &store,
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        server.uri(),
    );

    let accounts = service
        .warmup_due_accounts(8, 0, true)
        .await
        .expect("due accounts");
    let summary = service
        .execute_warmup("test-model", accounts)
        .await
        .expect("warmup");

    assert_eq!(summary.warmed_up, 1);
    let requests = server.received_requests().await.expect("recorded requests");
    let decoded =
        zstd::stream::decode_all(requests[0].body.as_slice()).expect("zstd body should decode");
    let body: serde_json::Value =
        serde_json::from_slice(&decoded).expect("warmup body should be valid JSON");
    // 上游按目录拒绝目录外的档位，none 曾让全部定时预热只留下 error 终态
    assert_eq!(body["reasoning"]["effort"], "low");
}

#[tokio::test]
async fn warmup_rejects_failed_sse_despite_successful_http_headers() {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: codex.rate_limits\n",
        "data: {\"type\":\"codex.rate_limits\",\"plan_type\":\"team\",\"rate_limits\":{\"allowed\":false,\"limit_reached\":true,\"primary\":{\"used_percent\":100,\"window_minutes\":300,\"reset_at\":1900000000}}}\n\n",
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\"}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-codex-allowed", "false")
                .set_body_raw(sse, "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let store = Arc::new(MemoryAccountStore::default());
    create_account(&store, "acct_warmup_failed").await;
    let service = quota_service_with_base_url(
        &store,
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        server.uri(),
    );

    let accounts = service
        .warmup_due_accounts(8, 0, true)
        .await
        .expect("due accounts");
    let summary = service
        .execute_warmup("test-model", accounts)
        .await
        .expect("warmup");

    assert_eq!(summary.warmed_up, 0);
    assert_eq!(summary.failed, 1);
    assert!(store.quota_json("acct_warmup_failed").is_none());
}

#[tokio::test]
async fn warmup_skips_expired_credential() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let store = Arc::new(MemoryAccountStore::default());
    create_account(&store, "acct_warmup_expired").await;
    let account = store.account("acct_warmup_expired").expect("account");
    persist_credential_state(&store, &account, CredentialState::Expired).await;
    let service = quota_service_with_base_url(
        &store,
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        server.uri(),
    );

    let accounts = service
        .warmup_due_accounts(8, 0, true)
        .await
        .expect("due accounts");
    let summary = service
        .execute_warmup("test-model", accounts)
        .await
        .expect("warmup");

    assert_eq!(summary.warmed_up, 0);
    assert_eq!(summary.failed, 0);
}

#[tokio::test]
async fn warmup_due_accounts_follow_account_note_schedule() {
    let store = Arc::new(MemoryAccountStore::default());
    for id in [
        "acct_note_global",
        "acct_note_invalid",
        "acct_note_off",
        "acct_note_own",
    ] {
        create_account(&store, id).await;
    }
    store.set_notes("acct_note_own", "主力号 预激活 = 07:30， 12:30；其他备注");
    store.set_notes("acct_note_off", "WARMUP=Off");
    store.set_notes("acct_note_invalid", "预激活=7:30");
    let service = quota_service(&store);

    // 单号时刻不依赖全局时间表，未声明与格式无效的账号只跟随全局时刻
    assert_eq!(
        due_account_ids(&service, 7, 30, false).await,
        ["acct_note_own"]
    );
    assert_eq!(
        due_account_ids(&service, 12, 30, true).await,
        ["acct_note_global", "acct_note_invalid", "acct_note_own"]
    );
    assert_eq!(
        due_account_ids(&service, 8, 0, true).await,
        ["acct_note_global", "acct_note_invalid"]
    );
    assert!(due_account_ids(&service, 8, 0, false).await.is_empty());
}

#[tokio::test]
async fn warmup_treats_full_window_placeholder_as_not_started() {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_warmup_placeholder\",\"status\":\"completed\",\"output\":[]}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let store = Arc::new(MemoryAccountStore::default());
    let observed_at = SystemTime::now() - Duration::from_secs(10 * 60);
    let observed = observed_at
        .duration_since(UNIX_EPOCH)
        .expect("epoch")
        .as_secs();
    // 上游对未开始的窗口返回 0 用量，重置时间为观测时刻加完整 5h
    seed_short_term_window(&store, "acct_window_placeholder", observed_at, observed + 18_000).await;
    // 同样 0 用量但观测时只剩 2h，说明窗口已经开始
    seed_short_term_window(&store, "acct_window_started", observed_at, observed + 7_200).await;
    let service = quota_service_with_base_url(
        &store,
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        server.uri(),
    );

    let accounts = service
        .warmup_due_accounts(8, 0, true)
        .await
        .expect("due accounts");
    let summary = service
        .execute_warmup("test-model", accounts)
        .await
        .expect("warmup");

    assert_eq!(summary.warmed_up, 1);
    assert_eq!(summary.skipped_active, 1);
    assert_eq!(summary.failed, 0);
}
