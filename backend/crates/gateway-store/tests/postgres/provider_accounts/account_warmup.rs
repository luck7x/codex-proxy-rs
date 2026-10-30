//! 验证账号预激活排期的持久化、审计与账号生命周期

use gateway_core::provider_ports::{AccountWarmupSchedule, ProviderRuntimePolicyPort as _};
use gateway_store::postgres::{PgRuntimeSettingsRepository, ensure_account_warmup_schema};

use super::*;

#[tokio::test]
async fn account_warmup_schedule_round_trips_audits_and_follows_account_deletion() {
    let Some(database) = TestDatabase::create("account_warmup").await else {
        return;
    };
    ensure_account_warmup_schema(&database.pool).await.unwrap();
    ensure_account_warmup_schema(&database.pool).await.unwrap();
    let repository = PgProviderAccountRepository::new(database.pool.clone());
    repository
        .insert_provider_account(account("acct_warmup", "user-warmup"))
        .await
        .unwrap();
    let store = admin_account_store(&database.pool);
    let policy = PgRuntimeSettingsRepository::new(database.pool.clone());
    let context = MutationContext {
        actor: MutationActor::System,
        request_id: "account-warmup-test".into(),
    };
    let schedule = AccountWarmupSchedule::try_new(vec!["11:02".into(), "06:00".into()]).unwrap();
    let disabled = AccountWarmupSchedule::try_new(Vec::new()).unwrap();
    let before = current_revision(&database.pool).await;

    let loaded = store.account_warmup_schedule("acct_warmup").await.unwrap();
    assert_eq!(loaded, None);
    for saved in [&schedule, &disabled, &schedule] {
        store
            .save_account_warmup_schedule("acct_warmup", Some(saved), &context)
            .await
            .unwrap();
        let loaded = store.account_warmup_schedule("acct_warmup").await.unwrap();
        assert_eq!(loaded.as_ref(), Some(saved));
    }
    let schedules = policy.load_account_warmup_schedules().await.unwrap();
    assert_eq!(schedules.get("acct_warmup"), Some(&schedule));
    assert!(current_revision(&database.pool).await > before);
    let audited: i64 = sqlx::query_scalar(
        "select count(*) from admin_audit_events where entity_ref = 'acct_warmup'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(audited, 3);

    store
        .save_account_warmup_schedule("acct_warmup", None, &context)
        .await
        .unwrap();
    let loaded = store.account_warmup_schedule("acct_warmup").await.unwrap();
    assert_eq!(loaded, None);

    store
        .save_account_warmup_schedule("acct_warmup", Some(&schedule), &context)
        .await
        .unwrap();
    sqlx::query("delete from provider_accounts where id = 'acct_warmup'")
        .execute(&database.pool)
        .await
        .unwrap();
    let schedules = policy.load_account_warmup_schedules().await.unwrap();
    assert!(schedules.is_empty());
    database.close().await;
}
