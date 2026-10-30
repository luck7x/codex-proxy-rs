//! 账号单独预激活排期的查询与保存

use gateway_core::provider_ports::AccountWarmupSchedule;

use super::*;
use crate::auth::SessionState;

/// `times` 为 `null` 跟随全局时间表，空数组表示不参与预激活
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetRequest {
    account_id: String,
    times: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    times: Option<Vec<String>>,
}

impl From<Option<AccountWarmupSchedule>> for Data {
    fn from(schedule: Option<AccountWarmupSchedule>) -> Self {
        Self {
            times: schedule.map(|schedule| schedule.times().to_vec()),
        }
    }
}

pub(super) fn router<S>() -> Router<S>
where
    S: SessionState + Clone + Send + Sync + 'static,
{
    Router::new().route("/api/admin/accounts/warmup", get(load::<S>).post(save::<S>))
}

async fn load<S>(
    _auth: AdminAuth,
    State(state): State<S>,
    AdminQuery(query): AdminQuery<AccountIdQuery>,
) -> Result<impl IntoResponse, AdminError>
where
    S: SessionState + Send + Sync,
{
    let schedule = state
        .admin_services()
        .accounts()
        .account_warmup_schedule(query.into_id().map_err(map_wire_error)?)
        .await
        .map_err(map_service_error)?;
    Ok(AdminResponse::new(
        StatusCode::OK,
        AdminEnvelope::ok(Data::from(schedule)),
    ))
}

async fn save<S>(
    auth: AdminAuth,
    State(state): State<S>,
    AdminJson(request): AdminJson<SetRequest>,
) -> Result<impl IntoResponse, AdminError>
where
    S: SessionState + Send + Sync,
{
    let account_id = AccountIdQuery {
        account_id: request.account_id,
    }
    .into_id()
    .map_err(map_wire_error)?;
    let schedule = state
        .admin_services()
        .accounts()
        .set_account_warmup_schedule(
            &auth.context().mutation_context(),
            account_id,
            request.times,
        )
        .await
        .map_err(map_service_error)?;
    Ok(AdminResponse::new(
        StatusCode::OK,
        AdminEnvelope::ok(Data::from(schedule)),
    ))
}
