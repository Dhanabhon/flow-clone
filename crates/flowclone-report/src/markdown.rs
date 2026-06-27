//! Markdown report renderer.

use crate::ReportData;

/// Render the report as a Markdown string.
pub fn render(data: &ReportData) -> String {
    let mut s = String::new();
    s.push_str("# FlowClone — Clone Report\n\n");

    if let Some(ts) = &data.started_at {
        s.push_str(&format!("**Started:** {ts}  \n"));
    }
    s.push_str(&format!("**Duration:** {:.2}s  \n", data.duration_secs));
    s.push_str(&format!(
        "**FlowClone version:** {}  \n\n",
        data.app_version
    ));

    s.push_str("## Source\n\n");
    s.push_str(&disk_section(&data.source));

    s.push_str("## Target\n\n");
    s.push_str(&disk_section(&data.target));

    s.push_str("## Throughput\n\n");
    s.push_str(&format!(
        "- Average speed: **{}/s**\n",
        humanspeed(data.average_speed)
    ));

    if let Some(v) = &data.verified {
        s.push_str("\n## Verification\n\n");
        s.push_str(&format!(
            "- Result: **{}**\n",
            if v.matched { "PASS" } else { "FAIL" }
        ));
        s.push_str(&format!("- Blocks checked: {}\n", v.blocks_checked));
        s.push_str(&format!("- Bytes checked: {}\n", v.bytes_checked));
    }

    if !data.warnings.is_empty() {
        s.push_str("\n## Warnings\n\n");
        for w in &data.warnings {
            s.push_str(&format!("- {w}\n"));
        }
    }

    s
}

fn disk_section(d: &flowclone_disk::DiskInfo) -> String {
    let mut s = String::new();
    s.push_str(&format!("- Device: `{}`\n", d.device_path));
    s.push_str(&format!("- Model: {}\n", d.model));
    if let Some(serial) = &d.serial {
        s.push_str(&format!("- Serial: {serial}\n"));
    }
    s.push_str(&format!("- Capacity: {} bytes\n", d.total_bytes));
    if let Some(fs) = &d.filesystem {
        s.push_str(&format!("- Filesystem: {fs}\n"));
    }
    s.push('\n');
    s
}

/// Turn a bytes/sec value into a friendly short string.
fn humanspeed(bytes_per_sec: u64) -> String {
    const UNITS: &[(&str, u64)] = &[("GB", 1_000_000_000), ("MB", 1_000_000), ("KB", 1_000)];
    for (unit, scale) in UNITS {
        if bytes_per_sec >= *scale {
            return format!("{:.1} {}", bytes_per_sec as f64 / *scale as f64, unit);
        }
    }
    format!("{bytes_per_sec} B")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReportData;
    use flowclone_disk::DiskInfo;

    fn sample() -> ReportData {
        ReportData {
            source: DiskInfo::placeholder("/dev/disk0"),
            target: DiskInfo::placeholder("/dev/disk1"),
            started_at: None,
            duration_secs: 12.0,
            average_speed: 500_000_000,
            verified: None,
            warnings: vec![],
            app_version: "0.1.0".into(),
        }
    }

    #[test]
    fn markdown_has_titles() {
        let md = render(&sample());
        assert!(md.contains("# FlowClone — Clone Report"));
        assert!(md.contains("## Source"));
        assert!(md.contains("## Target"));
    }

    #[test]
    fn human_speed_picks_mb() {
        assert_eq!(humanspeed(500_000_000), "500.0 MB");
    }
}
