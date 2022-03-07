
use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use proc_mounts::{MountInfo, MountIter};
use std::path::Path;
use std::time::SystemTime;
use regex::Regex;
use std::fs::DirEntry;

#[derive(Clone, Hash, PartialEq, Eq, Encode)]
pub(crate) struct BackupLabels {
    backups_disk: String,
    backups_path: String,
    backup_pattern: String
    //latest_backup_path: String
}

pub(crate) fn get_backup_freshness() -> Family<BackupLabels, Gauge> {
    Family::<BackupLabels, Gauge>::default()
}

pub(crate) fn measure_backup_freshness(mount_path: &str, bkp_pattern: &str, metric: &Family<BackupLabels, Gauge>) {

    let mount_point = MountIter::new().unwrap()
        .find(|m| match &m {
            Ok(MountInfo { ref dest, .. }) => mount_path.starts_with(dest.to_str().unwrap()),
            _ => false
        }).unwrap().ok().unwrap();

    let bkp_regex = Regex::new(bkp_pattern).unwrap();

    let mut backups: Vec<DirEntry> = Path::new(mount_path).read_dir().unwrap()
        .filter_map(|f| match f {
            Ok(d) if bkp_regex.is_match(d.file_name().to_str().unwrap()) => Some(d),
            _ => None
        })
        .collect();

    backups.sort_by_key(|bkp| {
        let metadata = bkp.metadata().unwrap();
        return metadata.created().unwrap_or(metadata.modified().unwrap());
    });


    let last_bkp_metadata = backups.last().unwrap().metadata().unwrap();
    let last_bkp_created = last_bkp_metadata.created()
        .unwrap_or(last_bkp_metadata.modified().unwrap());

    metric.get_or_create(&BackupLabels {
        backups_disk: mount_point.source.to_str().unwrap().parse().unwrap(),
        backups_path: mount_path.parse().unwrap(),
        backup_pattern: bkp_pattern.parse().unwrap()
        //latest_backup_path: backups.last().unwrap().path().to_str().unwrap().parse().unwrap()
    })
        .set(last_bkp_created.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs());
}