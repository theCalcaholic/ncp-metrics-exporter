use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use proc_mounts::{MountInfo, MountIter};
use std::path::Path;
use std::time::SystemTime;
use regex::{Regex};
use std::fs::DirEntry;
use chrono::{DateTime, TimeZone};
use chrono::prelude::Local;
use failure::{Error};

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

pub(crate) fn measure_backup_freshness(mount_path: &str, bkp_pattern: &str, metric: &Family<BackupLabels, Gauge>) -> Result<(), Error> {

    let mount_point = MountIter::new()?
        .find(|m| match &m {
            Ok(MountInfo { ref dest, .. }) => mount_path.starts_with(dest.to_str().unwrap()),
            _ => false
        }).expect(&*format!("Could not find any mount point containing '{}'", mount_path))?;

    let bkp_regex = Regex::new(bkp_pattern)
        .expect(&*format!("Invalid backup file pattern: '{}'", bkp_pattern));

    let mut backups: Vec<DirEntry> = Path::new(mount_path).read_dir()
        .expect(&*format!("Could not read directory contents for {}", mount_path))
        .filter_map(|f| match f {
            Ok(d) if bkp_regex.is_match(d.file_name().to_str()?) => Some(d),
            _ => None
        })
        .collect();

    backups.sort_by_key(move |bkp| {
        let metadata = &bkp.metadata()
            .expect(&*format!("Could not retrieve metadata for {:#?}", &bkp.file_name()));
        let file_name = bkp.file_name().into_string()
            .expect(&*format!("Failed to get file name for backup {:#?}", &bkp));

        match date_from_file_name(&file_name, bkp_pattern) {
            Some(dt) => SystemTime::from(dt),
            None => metadata.created().or_else(|_| metadata.modified())
                .expect(&*format!("Could not retrieve date time from backup {:#?}", &bkp))
        }
    });



    let last_bkp_created = match backups.last() {
        None => Ok(SystemTime::UNIX_EPOCH),
        Some(bkp) => {
            let file_name = bkp.file_name().into_string()
                .expect(&*format!("Failed to get file name for backup {:#?}", bkp));
            match date_from_file_name(&file_name, bkp_pattern) {
                Some(dt) => Ok(SystemTime::from(dt)),
                None => match bkp.metadata() {
                    Ok(metadata) => Ok(metadata.created().unwrap_or(metadata.modified()?)),
                    Err(_) => Err(format!("Failed to retrieve backup metadata for {:#?}", bkp))
                }
            }
        }}.expect("Failed to extract latest backup creation date");

    metric.get_or_create(&BackupLabels {
        backups_disk: mount_point.source.to_str()
            .expect("Error extracting mount point source").parse()?,
        backups_path: mount_path.parse()?,
        backup_pattern: bkp_pattern.parse()?
    })
        .set(SystemTime::now().duration_since(last_bkp_created)?.as_secs());
    Ok(())
}

fn date_from_file_name(file_name: &str, pattern: &str) -> Option<DateTime<Local>> {
    let regex = Regex::new(pattern).unwrap();
    match regex.captures(&*file_name)
    {
        Some(cap) => {
            let date_values = [
                cap.name("year"), cap.name("month"), cap.name("day"),
                cap.name("hour"), cap.name("minute"), cap.name("second")
            ]
                .map(|o| match o {
                    Some(m) => m.as_str().parse::<u32>().ok(),
                    None => None
                });

            match date_values[..] {
                [Some(year), Some(month), Some(day), hour, minute, second] => {
                    Some(Local.ymd(year as i32, month, day)
                        .and_hms(hour.unwrap_or(0),
                                 minute.unwrap_or(0),
                                 second.unwrap_or(0)))
                },
                _ => None
            }
        }
        _ => None
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, Timelike};
    use super::*;

    fn assert_date_equals(dt: DateTime<Local>, values: (i32, u32, u32, u32, u32, u32)) {
        let (year, month, day, hour, minute, second) = values;
        let date = dt.date();
        assert_eq!(date.year(), year);
        assert_eq!(date.month(), month);
        assert_eq!(date.day(), day);

        let time = dt.time();
        assert_eq!(time.hour(), hour);
        assert_eq!(time.minute(), minute);
        assert_eq!(time.second(), second);
    }

    #[test]
    fn test_date_from_file_name() {

        // Case 1: successful extraction without time
        let mut pattern = r".*-(?P<year>\d+)-(?P<month>\d+)-(?P<day>\d+)\.ext";
        let mut file_name = "testfile-2022-03-11.ext";

        let mut dt = date_from_file_name(file_name, pattern);
        assert!(dt.is_some());
        assert_date_equals(dt.unwrap(), (2022, 03, 11, 0, 0, 0));

        // Case 2: successful extraction with time
        pattern = r".*-(?P<year>\d+)-(?P<month>\d+)-(?P<day>\d+)_(?P<hour>\d{2}):(?P<minute>\d{2}):(?P<second>\d{2}).ext";
        file_name = "testfile-2022-03-11_10:08:22.ext";
        dt = date_from_file_name(file_name, pattern);

        assert!(dt.is_some());
        assert_date_equals(dt.unwrap(), (2022, 03, 11, 10, 8, 22));

        // Case 3: invalid file name (not an int)

        pattern = pattern;
        file_name = "testfile-2022-OCT-11_10:08:22.ext";
        dt = date_from_file_name(file_name, pattern);
        assert!(dt.is_none());

        // Case 4: file name doesn't match pattern

        pattern = pattern;
        file_name = "testfile-2022-08.ext";
        dt = date_from_file_name(file_name, pattern);
        assert!(dt.is_none());


    }
}