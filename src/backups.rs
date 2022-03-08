
use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use proc_mounts::{MountInfo, MountIter};
use std::path::Path;
use std::time::SystemTime;
use regex::{Captures, Match, Regex};
use std::fs::DirEntry;
use chrono::{DateTime, TimeZone};
use chrono::prelude::Local;

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
        let metadata_time = metadata.created().unwrap_or(metadata.modified().unwrap());

        match date_from_file_name(bkp.file_name().to_str().unwrap().parse().unwrap(), bkp_pattern) {
            Some(dt) => SystemTime::from(dt),
            None => metadata_time
        }
    });

    if backups.is_empty() {
        return;
    }


    let last_bkp_created = match date_from_file_name(
        backups.last().unwrap().file_name().to_str().unwrap().parse().unwrap(),
        bkp_pattern
    ) {
        Some(dt) => SystemTime::from(dt),
        None => {
            let last_bkp_metadata = backups.last().unwrap().metadata().unwrap();
            last_bkp_metadata
                .created()
                .unwrap_or(last_bkp_metadata.modified().unwrap())
        }
    };

    metric.get_or_create(&BackupLabels {
        backups_disk: mount_point.source.to_str().unwrap().parse().unwrap(),
        backups_path: mount_path.parse().unwrap(),
        backup_pattern: bkp_pattern.parse().unwrap()
        //latest_backup_path: backups.last().unwrap().path().to_str().unwrap().parse().unwrap()
    })
        .set(SystemTime::now().duration_since(last_bkp_created).unwrap().as_secs());
}

fn date_from_file_name(file_name: String, pattern: &str) -> Option<DateTime<Local>> {
    let regex = Regex::new(pattern).unwrap();
    match regex.captures(&*file_name)
    {
        Some(cap) => {
            let year = cap.name("year");
            let month = cap.name("month");
            let day = cap.name("day");
            let hour = cap.name("hour");
            let minute = cap.name("minute");
            let second = cap.name("second");
            if vec![year, month, day].iter().all(|g| g.is_some()) {
                let dt = Local.ymd(year.unwrap().as_str().parse().unwrap(),
                          month.unwrap().as_str().parse().unwrap(),
                          day.unwrap().as_str().parse().unwrap())
                    .and_hms(
                        hour.map_or(0, |m| m.as_str().parse().unwrap()),
                        minute.map_or(0, |m| m.as_str().parse().unwrap()),
                        second.map_or(0, |m|  m.as_str().parse().unwrap()));
                return Some(dt);
            }
            None
        }
        _ => None
    }
}