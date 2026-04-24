use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use super::score::{RetentionTier, SensitivityMode, Thresholds, calculate_freshness, score_to_status};

use serde::{Serialize, Deserialize};

pub struct FreshnessService {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FreshnessReport {
    pub total_docs: i64,
    pub fresh_docs: i64,
    pub aging_docs: i64,
    pub stale_docs: i64,
    pub obsolete_docs: i64,
    pub average_score: f64,
}

impl FreshnessService {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Recalculates all scores in the database
    pub fn recalculate_all(&self) -> Result<usize, rusqlite::Error> {
        self.recalculate_internal(None)
    }

    /// Recalculates scores for a specific project
    pub fn recalculate_project(&self, project_id: i64) -> Result<usize, rusqlite::Error> {
        self.recalculate_internal(Some(project_id))
    }

    /// Recalculates scores for a specific document
    pub fn recalculate_document(&self, doc_id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        
        let mut stmt = conn.prepare("
            SELECT df.document_id, df.last_content_change, df.retention_tier, p.freshness_policy_json 
            FROM document_freshness df
            JOIN documents d ON df.document_id = d.id
            JOIN projects p ON d.project_id = p.id
            WHERE df.document_id = ?1
        ")?;
        
        if let Some(row) = stmt.query_row([doc_id], |row| {
            let doc_id: i64 = row.get(0)?;
            let last_change: Option<i64> = row.get(1)?;
            let last_change_ts = last_change.unwrap_or(now); 
            let tier_str: String = row.get(2)?;
            let tier = RetentionTier::from_str(&tier_str);
            let policy_json: Option<String> = row.get(3)?;
            
            let mut mode = SensitivityMode::Normal;
            if let Some(json_str) = policy_json {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(m) = val.get("sensitivity_mode").and_then(|v| v.as_str()) {
                        mode = match m.to_lowercase().as_str() {
                            "strict" => SensitivityMode::Strict,
                            "relaxed" => SensitivityMode::Relaxed,
                            _ => SensitivityMode::Normal,
                        };
                    }
                }
            }
            
            let days_since = ((now - last_change_ts) as f64) / 86400.0;
            let score = calculate_freshness(days_since, tier, mode);
            let status = score_to_status(score, &Thresholds::default());
            
            Ok((doc_id, score, status.as_str().to_string()))
        }).ok() {
            let (id, score, status) = row;
            conn.execute(
                "UPDATE document_freshness SET freshness_score = ?1, status = ?2, score_updated_at = ?3 WHERE document_id = ?4",
                rusqlite::params![score, status, now, id],
            )?;
        }
        
        Ok(())
    }

    fn recalculate_internal(&self, project_id: Option<i64>) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        
        let mut sql = "
            SELECT df.document_id, df.last_content_change, df.retention_tier, p.freshness_policy_json 
            FROM document_freshness df
            JOIN documents d ON df.document_id = d.id
            JOIN projects p ON d.project_id = p.id
        ".to_string();
        
        let mut params = Vec::new();
        if let Some(pid) = project_id {
            sql.push_str(" WHERE d.project_id = ?1");
            params.push(pid);
        }
        
        let mut stmt = conn.prepare(&sql)?;
        
        let doc_updates: Vec<(i64, f64, String)> = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            let doc_id: i64 = row.get(0)?;
            let last_change: Option<i64> = row.get(1)?;
            let last_change_ts = last_change.unwrap_or(now); 
            let tier_str: String = row.get(2)?;
            let tier = RetentionTier::from_str(&tier_str);
            let policy_json: Option<String> = row.get(3)?;
            
            let mut mode = SensitivityMode::Normal;
            if let Some(json_str) = policy_json {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(m) = val.get("sensitivity_mode").and_then(|v| v.as_str()) {
                        mode = match m.to_lowercase().as_str() {
                            "strict" => SensitivityMode::Strict,
                            "relaxed" => SensitivityMode::Relaxed,
                            _ => SensitivityMode::Normal,
                        };
                    }
                }
            }
            
            let days_since = ((now - last_change_ts) as f64) / 86400.0;
            let score = calculate_freshness(days_since, tier, mode);
            let status = score_to_status(score, &Thresholds::default());
            
            Ok((doc_id, score, status.as_str().to_string()))
        })?.filter_map(Result::ok).collect();
        
        let mut count = 0;
        for (doc_id, score, status) in doc_updates {
            conn.execute(
                "UPDATE document_freshness SET freshness_score = ?1, status = ?2, score_updated_at = ?3 WHERE document_id = ?4",
                rusqlite::params![score, status, now, doc_id],
            )?;
            count += 1;
        }
        
        Ok(count)
    }

    pub fn get_stale_docs(&self, project_id: Option<i64>, limit: u32) -> Result<Vec<i64>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut sql = "SELECT df.document_id FROM document_freshness df 
                       JOIN documents d ON df.document_id = d.id 
                       WHERE df.status IN ('stale', 'aging')".to_string();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        
        if let Some(pid) = project_id {
            sql.push_str(" AND d.project_id = ?1");
            params.push(rusqlite::types::Value::Integer(pid));
        }
        
        sql.push_str(" ORDER BY df.freshness_score ASC LIMIT ?");
        params.push(rusqlite::types::Value::Integer(limit as i64));
        
        let mut stmt = conn.prepare(&sql)?;
        let ids: Vec<i64> = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            row.get(0)
        })?.filter_map(Result::ok).collect();
        
        Ok(ids)
    }

    /// Generates an aggregated freshness report for a project (or all if None)
    pub fn get_project_freshness_report(&self, project_id: Option<i64>) -> Result<FreshnessReport, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        
        let mut sql = "SELECT 
            COUNT(d.id) as total,
            SUM(CASE WHEN df.status = 'fresh' THEN 1 ELSE 0 END) as fresh_count,
            SUM(CASE WHEN df.status = 'aging' THEN 1 ELSE 0 END) as aging_count,
            SUM(CASE WHEN df.status = 'stale' THEN 1 ELSE 0 END) as stale_count,
            SUM(CASE WHEN df.status = 'obsolete' THEN 1 ELSE 0 END) as obsolete_count,
            AVG(df.freshness_score) as avg_score
            FROM document_freshness df
            JOIN documents d ON df.document_id = d.id"
            .to_string();
            
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(pid) = project_id {
            sql.push_str(" WHERE d.project_id = ?1");
            params.push(rusqlite::types::Value::Integer(pid));
        }
        
        let mut stmt = conn.prepare(&sql)?;
        let report = stmt.query_row(rusqlite::params_from_iter(params), |r| {
            Ok(FreshnessReport {
                total_docs: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                fresh_docs: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                aging_docs: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                stale_docs: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                obsolete_docs: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                average_score: r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
            })
        })?;
        
        Ok(report)
    }

    /// Updates tier for a document
    pub fn update_document_freshness_config(
        &self, 
        project_id: i64, 
        source_doc_id: &str, 
        tier: Option<&str>
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        
        // Find document_id
        let doc_id: i64 = match conn.query_row(
            "SELECT id FROM documents WHERE project_id = ?1 AND source_doc_id = ?2",
            rusqlite::params![project_id, source_doc_id],
            |r| r.get(0)
        ) {
            Ok(id) => id,
            Err(_) => return Ok(false), // Document not found
        };
        
        if let Some(t) = tier {
            conn.execute(
                "UPDATE document_freshness SET retention_tier = ?1, tier_source = 'user' WHERE document_id = ?2",
                rusqlite::params![t, doc_id]
            )?;
            
            // 즉시 재계산 트리거 (Lock 주의: recalculate_document가 다시 락을 잡으므로 여기서는 드롭 필요)
            drop(conn); 
            let _ = self.recalculate_document(doc_id);
        }
        
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;

    #[test]
    fn test_recalculate_all() {
        let db = TestDb::new();
        let service = FreshnessService::new(Arc::new(Mutex::new(db.conn)));
        // Note: Without inserting fake documents, count is 0
        let count = service.recalculate_all().unwrap();
        assert_eq!(count, 0);
    }
}
