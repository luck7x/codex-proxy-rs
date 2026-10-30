//! 账号预激活排期的 PostgreSQL 读写
//!
//! 排期表不进入冻结迁移集合：热补丁与上游迁移编号互不影响，回退官方版本时也不会因
//! 缺少迁移文件而启动失败；官方版本不读取这张表

use std::collections::BTreeMap;

use gateway_admin::model::audit::MutationAuditOperation;
use gateway_core::provider_ports::{
    AccountWarmupSchedule, ProviderStoreError, ProviderStoreErrorKind,
};

use super::*;

/// 没有记录的账号跟随全局时间表，`schedule_time` 为空表示不参与预激活
pub async fn ensure_account_warmup_schema(pool: &PgPool) -> StoreResult<()> {
    sqlx::query(
        r"create table if not exists luck7x_account_warmup (
            account_id text primary key references provider_accounts (id) on delete cascade,
            schedule_time text not null check (
              schedule_time ~ '^(([01][0-9]|2[0-3]):[0-5][0-9](,([01][0-9]|2[0-3]):[0-5][0-9])*)?$'
            )
          )",
    )
    .execute(pool)
    .await
    .map_err(|source| postgres_unavailable("ensure account warmup schema", source))?;
    Ok(())
}

pub(in crate::postgres) async fn load_account_warmup_schedules(
    pool: &PgPool,
) -> Result<BTreeMap<String, AccountWarmupSchedule>, ProviderStoreError> {
    sqlx::query_as::<_, (String, String)>(
        "select account_id, schedule_time from luck7x_account_warmup",
    )
    .fetch_all(pool)
    .await
    .map_err(|source| crate::provider_unavailable("load account warmup schedules", source))?
    .into_iter()
    .map(|(account_id, stored)| {
        let schedule = AccountWarmupSchedule::from_stored(&stored).map_err(|_| {
            ProviderStoreError::new(
                ProviderStoreErrorKind::InvalidData,
                "decode account warmup schedule",
            )
        })?;
        Ok((account_id, schedule))
    })
    .collect()
}

impl PgAdminAccountStore {
    pub(super) async fn load_account_warmup(
        &self,
        account_id: &str,
    ) -> AdminStoreResult<Option<AccountWarmupSchedule>> {
        let stored = sqlx::query_scalar::<_, String>(
            "select schedule_time from luck7x_account_warmup where account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|source| warmup_error("load account warmup schedule", source))?;
        stored
            .map(|stored| {
                AccountWarmupSchedule::from_stored(&stored).map_err(|message| {
                    AdminStoreError::new(AdminStoreErrorKind::Invalid, "account warmup", message)
                })
            })
            .transpose()
    }

    pub(super) async fn store_account_warmup(
        &self,
        account_id: &str,
        schedule: Option<&AccountWarmupSchedule>,
        context: &MutationContext,
    ) -> AdminStoreResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|source| warmup_error("begin account warmup", source))?;
        // 配置写入统一先锁全局版本，再锁账号记录
        let revision = bump_config_revision_in_transaction(&mut tx)
            .await
            .map_err(|error| admin_store_error("account warmup", error))?;
        let write = match schedule {
            Some(schedule) => sqlx::query(
                "insert into luck7x_account_warmup (account_id, schedule_time) values ($1, $2)
                 on conflict (account_id) do update set schedule_time = excluded.schedule_time",
            )
            .bind(account_id)
            .bind(schedule.to_stored()),
            None => sqlx::query("delete from luck7x_account_warmup where account_id = $1")
                .bind(account_id),
        };
        write
            .execute(&mut *tx)
            .await
            .map_err(|source| warmup_error("save account warmup schedule", source))?;
        append_admin_audit_event_in_transaction(
            &mut tx,
            mutation_audit(
                context,
                MutationAuditOperation::ProviderAccountUpdate,
                account_id,
                vec!["account_warmup".to_owned()],
            ),
            revision,
        )
        .await
        .map_err(|error| admin_store_error("account warmup", error))?;
        tx.commit()
            .await
            .map_err(|source| warmup_error("commit account warmup", source))
    }
}

fn warmup_error(operation: &'static str, source: sqlx::Error) -> AdminStoreError {
    admin_store_error("account warmup", postgres_unavailable(operation, source))
}
