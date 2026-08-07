use chrono::Utc;

pub fn now_ts() -> i64 {
    Utc::now().timestamp()
}
