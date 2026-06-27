//! JSON report renderer.

use crate::ReportData;

/// Render the report as a pretty-printed JSON string.
pub fn render(data: &ReportData) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowclone_disk::DiskInfo;

    #[test]
    fn json_is_valid_object() {
        let data = ReportData {
            source: DiskInfo::placeholder("/dev/disk0"),
            target: DiskInfo::placeholder("/dev/disk1"),
            started_at: None,
            duration_secs: 1.0,
            average_speed: 0,
            verified: None,
            warnings: vec![],
            app_version: "0.1.0".into(),
        };
        let s = render(&data).unwrap();
        assert!(s.trim_start().starts_with('{'));
        assert!(s.contains("\"app_version\""));
    }
}
