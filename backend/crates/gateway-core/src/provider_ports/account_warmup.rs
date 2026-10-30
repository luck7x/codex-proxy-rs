//! 账号单独的预激活排期，没有排期的账号跟随全局时间表

use super::valid_warmup_schedule_time;

const MAX_ACCOUNT_WARMUP_TIMES: usize = 8;

/// 账号自身的每日预激活时刻，按时间升序且不重复；空列表表示该账号不参与预激活
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountWarmupSchedule {
    times: Vec<String>,
}

impl AccountWarmupSchedule {
    /// 管理写入与持久化读取共用的时刻约束
    pub fn try_new(mut times: Vec<String>) -> Result<Self, &'static str> {
        if times
            .iter()
            .any(|time| time.len() != 5 || !valid_warmup_schedule_time(time))
        {
            return Err("预激活时间格式应为 HH:MM");
        }
        times.sort_unstable();
        times.dedup();
        if times.len() > MAX_ACCOUNT_WARMUP_TIMES {
            return Err("单个账号最多设置 8 个预激活时间");
        }
        Ok(Self { times })
    }

    /// 解析持久化的逗号列表，空字符串表示不参与预激活
    pub fn from_stored(value: &str) -> Result<Self, &'static str> {
        if value.is_empty() {
            return Self::try_new(Vec::new());
        }
        Self::try_new(value.split(',').map(str::to_owned).collect())
    }

    #[must_use]
    pub fn to_stored(&self) -> String {
        self.times.join(",")
    }

    #[must_use]
    pub fn times(&self) -> &[String] {
        &self.times
    }

    #[must_use]
    pub fn is_due(&self, hour: u32, minute: u32) -> bool {
        self.times.contains(&format!("{hour:02}:{minute:02}"))
    }
}
