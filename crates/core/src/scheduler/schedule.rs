use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, TimeZone, Datelike, NaiveTime, Duration};

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
        let now = Utc.timestamp_opt(now_epoch, 0).unwrap();
        
        match self {
            Schedule::Interval { seconds } => {
                now_epoch + (*seconds as i64)
            }
            Schedule::Daily { hour, minute } => {
                let target_time = NaiveTime::from_hms_opt(*hour, *minute, 0).unwrap();
                let today_target = now.date_naive().and_time(target_time);
                let today_target_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_target, Utc);
                
                if today_target_utc > now {
                    today_target_utc.timestamp()
                } else {
                    today_target_utc.timestamp() + 86400 // Add one day
                }
            }
            Schedule::Weekly { day_of_week, hour, minute } => {
                let target_time = NaiveTime::from_hms_opt(*hour, *minute, 0).unwrap();
                let today_target = now.date_naive().and_time(target_time);
                let mut target_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_target, Utc);
                
                // Adjust day of week
                let current_dow = now.weekday().num_days_from_sunday();
                let days_to_add = if *day_of_week > current_dow {
                    *day_of_week - current_dow
                } else if *day_of_week == current_dow && target_utc > now {
                    0
                } else {
                    7 - (current_dow - *day_of_week)
                };
                
                if days_to_add > 0 {
                    target_utc = target_utc + Duration::days(days_to_add as i64);
                }
                
                target_utc.timestamp()
            }
            Schedule::Monthly { day_of_month, hour, minute } => {
                let target_time = NaiveTime::from_hms_opt(*hour, *minute, 0).unwrap();
                let mut target_day = *day_of_month;
                
                // Adjust for months with fewer days if we pass 28/29/30
                // For simplicity, naive adjustment: if current month has fewer days than target_day, we cap it or we just use the naive date builder which might panic if overflow. 
                // A better approach is using exact arithmetic. Let's handle it safely.
                let mut year = now.year();
                let mut month = now.month();
                
                // clamp day_of_month to valid days in the month
                target_day = target_day.min(get_days_in_month(year, month));
                let mut target_date = chrono::NaiveDate::from_ymd_opt(year, month, target_day).unwrap();
                let mut target_utc = DateTime::<Utc>::from_naive_utc_and_offset(target_date.and_time(target_time), Utc);
                
                if target_utc <= now {
                    // Next month
                    if month == 12 {
                        month = 1;
                        year += 1;
                    } else {
                        month += 1;
                    }
                    target_day = (*day_of_month).min(get_days_in_month(year, month));
                    target_date = chrono::NaiveDate::from_ymd_opt(year, month, target_day).unwrap();
                    target_utc = DateTime::<Utc>::from_naive_utc_and_offset(target_date.and_time(target_time), Utc);
                }
                
                target_utc.timestamp()
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
    pub executor: Executor,
    pub action: String,
    pub action_config: serde_json::Value,
    pub schedule: Schedule,
    pub enabled: bool,
    pub run_on_idle: bool,
    pub last_run_at: Option<i64>,
    pub next_run_at: i64,
    pub created_by: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_interval() {
        let sc = Schedule::Interval { seconds: 60 };
        let now = 1000;
        assert_eq!(sc.next_run_after(now), 1060);
    }

    #[test]
    fn test_daily() {
        let sc = Schedule::Daily { hour: 4, minute: 30 };
        let now = Utc.with_ymd_and_hms(2026, 4, 24, 3, 0, 0).unwrap().timestamp();
        let expected = Utc.with_ymd_and_hms(2026, 4, 24, 4, 30, 0).unwrap().timestamp();
        assert_eq!(sc.next_run_after(now), expected);

        let now_past = Utc.with_ymd_and_hms(2026, 4, 24, 5, 0, 0).unwrap().timestamp();
        let expected_next_day = Utc.with_ymd_and_hms(2026, 4, 25, 4, 30, 0).unwrap().timestamp();
        assert_eq!(sc.next_run_after(now_past), expected_next_day);
    }

    #[test]
    fn test_weekly() {
        // day_of_week: 0 = Sunday
        // 2026-04-24 is Friday (5)
        let sc = Schedule::Weekly { day_of_week: 0, hour: 10, minute: 0 };
        let now = Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap().timestamp();
        let expected = Utc.with_ymd_and_hms(2026, 4, 26, 10, 0, 0).unwrap().timestamp(); // Sunday 26th
        assert_eq!(sc.next_run_after(now), expected);
    }
}
