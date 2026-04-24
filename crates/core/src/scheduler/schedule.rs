use serde::{Deserialize, Serialize};
use chrono::{TimeZone, Datelike, NaiveTime, Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Schedule {
    Interval { seconds: u64 },
    Daily { hour: u32, minute: u32 },
    Weekly { day_of_week: u32, hour: u32, minute: u32 }, // 0 = Sunday, 1 = Monday... 6 = Saturday
    Monthly { day_of_month: u32, hour: u32, minute: u32 }, // 1..31
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule::Daily { hour: 3, minute: 0 }
    }
}

impl Schedule {
    /// Calculate the next run time after the given epoch time (in seconds).
    pub fn next_run_after(&self, now_epoch: i64) -> i64 {
        use chrono::Local;
        let now = Local.timestamp_opt(now_epoch, 0).unwrap();
        
        match self {
            Schedule::Interval { seconds } => {
                now_epoch + (*seconds as i64)
            }
            Schedule::Daily { hour, minute } => {
                let target_time = NaiveTime::from_hms_opt(*hour, *minute, 0).unwrap();
                let today_target = now.date_naive().and_time(target_time);
                
                if let Some(target_local) = Local.from_local_datetime(&today_target).single() {
                    if target_local > now {
                        target_local.timestamp()
                    } else {
                        (target_local + Duration::days(1)).timestamp()
                    }
                } else {
                    // Fallback for DST transitions or other anomalies
                    now_epoch + 86400
                }
            }
            Schedule::Weekly { day_of_week, hour, minute } => {
                let target_time = NaiveTime::from_hms_opt(*hour, *minute, 0).unwrap();
                let today_target = now.date_naive().and_time(target_time);
                
                if let Some(target_local_orig) = Local.from_local_datetime(&today_target).single() {
                    let current_dow = now.weekday().num_days_from_sunday();
                    let days_to_add = if *day_of_week > current_dow {
                        *day_of_week - current_dow
                    } else if *day_of_week == current_dow && target_local_orig > now {
                        0
                    } else {
                        7 - (current_dow - *day_of_week)
                    };
                    
                    (target_local_orig + Duration::days(days_to_add as i64)).timestamp()
                } else {
                    now_epoch + 7 * 86400
                }
            }
            Schedule::Monthly { day_of_month, hour, minute } => {
                let target_time = NaiveTime::from_hms_opt(*hour, *minute, 0).unwrap();
                let mut year = now.year();
                let mut month = now.month();
                
                let target_day = (*day_of_month).min(get_days_in_month(year, month));
                let target_date = chrono::NaiveDate::from_ymd_opt(year, month, target_day).unwrap();
                let today_target = target_date.and_time(target_time);
                
                if let Some(mut target_local) = Local.from_local_datetime(&today_target).single() {
                    if target_local <= now {
                        // Next month
                        if month == 12 {
                            month = 1;
                            year += 1;
                        } else {
                            month += 1;
                        }
                        let next_target_day = (*day_of_month).min(get_days_in_month(year, month));
                        let next_target_date = chrono::NaiveDate::from_ymd_opt(year, month, next_target_day).unwrap();
                        target_local = Local.from_local_datetime(&next_target_date.and_time(target_time)).single().unwrap();
                    }
                    target_local.timestamp()
                } else {
                    now_epoch + 30 * 86400
                }
            }
        }
    }
}

fn get_days_in_month(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    System,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: i64,
    pub project_id: Option<i64>,
    pub job_name: String,
    pub description: Option<String>,
    pub executor: Executor,
    pub action: String,
    pub action_config: serde_json::Value,
    pub schedule: Schedule,
    pub enabled: bool,
    pub run_on_idle: bool,
    pub is_immutable: bool,
    pub last_run_at: Option<i64>,
    pub next_run_at: i64,
    pub created_by: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Local};

    #[test]
    fn test_interval() {
        let sc = Schedule::Interval { seconds: 60 };
        let now = 1000;
        assert_eq!(sc.next_run_after(now), 1060);
    }

    #[test]
    fn test_daily() {
        let sc = Schedule::Daily { hour: 4, minute: 30 };
        let now = Local.with_ymd_and_hms(2026, 4, 24, 3, 0, 0).unwrap().timestamp();
        let expected = Local.with_ymd_and_hms(2026, 4, 24, 4, 30, 0).unwrap().timestamp();
        assert_eq!(sc.next_run_after(now), expected);

        let now_past = Local.with_ymd_and_hms(2026, 4, 24, 5, 0, 0).unwrap().timestamp();
        let expected_next_day = Local.with_ymd_and_hms(2026, 4, 25, 4, 30, 0).unwrap().timestamp();
        assert_eq!(sc.next_run_after(now_past), expected_next_day);
    }

    #[test]
    fn test_weekly() {
        // 2026-04-24 is Friday (5)
        let sc = Schedule::Weekly { day_of_week: 0, hour: 10, minute: 0 };
        let now = Local.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap().timestamp();
        let expected = Local.with_ymd_and_hms(2026, 4, 26, 10, 0, 0).unwrap().timestamp(); // Sunday 26th
        assert_eq!(sc.next_run_after(now), expected);
    }
}
