//! 验证账号预激活排期的时刻约束与持久化格式

use gateway_core::provider_ports::AccountWarmupSchedule;

fn times(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn account_warmup_schedule_sorts_and_deduplicates_times() {
    let schedule = AccountWarmupSchedule::try_new(times(&["11:02", "06:00", "11:02"])).unwrap();

    assert_eq!(schedule.times(), ["06:00", "11:02"]);
    assert_eq!(schedule.to_stored(), "06:00,11:02");
    assert!(schedule.is_due(6, 0));
    assert!(!schedule.is_due(6, 1));
}

#[test]
fn account_warmup_schedule_rejects_invalid_times() {
    for value in ["8:00", "24:00", "08:00,09:00", ""] {
        assert!(AccountWarmupSchedule::try_new(times(&[value])).is_err());
    }
    let too_many = (0..9).map(|hour| format!("{hour:02}:00")).collect();
    assert!(AccountWarmupSchedule::try_new(too_many).is_err());
}

#[test]
fn account_warmup_schedule_stores_empty_as_disabled() {
    let disabled = AccountWarmupSchedule::from_stored("").unwrap();

    assert!(disabled.times().is_empty());
    assert_eq!(disabled.to_stored(), "");
    assert!(AccountWarmupSchedule::from_stored("06:00,").is_err());
}
