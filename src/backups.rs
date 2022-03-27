use std::fmt;
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
use failure::{Error as FError};

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



pub(crate) fn measure_backup_freshness(mount_path: &str, bkp_pattern: &str, metric: &Family<BackupLabels, Gauge>) -> Result<(), FError> {

    let mount_point = find_mount_point(mount_path)?;

    let last_bkp_created = find_latest_backup_time(bkp_pattern, mount_path)?;

    metric.get_or_create(&BackupLabels {
        backups_disk: mount_point.source.to_str()
            .expect("Error extracting mount point source").parse()?,
        backups_path: mount_path.parse()?,
        backup_pattern: bkp_pattern.parse()?
    })
        .set(SystemTime::now().duration_since(last_bkp_created)?.as_secs());
    Ok(())
}

fn find_mount_point(mount_path: &str) -> Result<MountInfo, MountPointParsingError> {

    let mut mount_points = MountIter::new()?
        .filter(|m| match m {
            Ok(MountInfo { ref dest, .. }) => mount_path.starts_with(dest.to_str().unwrap()),
            _ => false
        }).filter_map(|r| r.ok())
        .collect::<Vec<MountInfo>>();

    // Use longest match (nested mountpoints are a thing :P)
    mount_points.pop().ok_or(MountPointParsingError{
        mount_path: format!("Could not find any mount point containing '{}'", mount_path),
        inner: None
    })
}

fn find_latest_backup_time(bkp_pattern: &str, mount_path: &str) -> Result<SystemTime, BackupParsingError> {
    let bkp_regex = Regex::new(bkp_pattern)
        .expect(&*format!("Invalid backup file pattern: '{}'", bkp_pattern));

    let mut backups: Vec<DirEntry> = match Path::new(mount_path).read_dir() {
        Ok(dir) => {
            dir.filter_map(| f | match f {
                Ok(d) if bkp_regex.is_match(d.file_name().to_str() ? ) => Some(d),
                _ => None
            }).collect()
        },
        // Err(e) => Err(format!("Could not read directory contents for {}", mount_path))
        Err(_e) => vec![]
    };

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

    if backups.is_empty() {
        return Err(BackupParsingError{message: format!("No backups found matching {}/{}",
                                                       mount_path, bkp_pattern)})
    }

    match backups.last() {
        None => Ok(SystemTime::UNIX_EPOCH),
        Some(bkp) => {
            let file_name = bkp.file_name().into_string()
                .expect(&*format!("Failed to get file name for backup {:#?}", bkp));
            match date_from_file_name(&file_name, bkp_pattern) {
                Some(dt) => Ok(SystemTime::from(dt)),
                None => match bkp.metadata() {
                    Ok(metadata) => Ok(metadata.created().unwrap_or(metadata.modified()?)),
                    Err(_) => Err(BackupParsingError{
                        message: format!("Failed to retrieve backup metadata for {:#?}", bkp)
                    })
                }
            }
        }}
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

#[derive(Debug, Clone)]
struct MountPointParsingError {
    mount_path: String,
    inner: Option<String>
}

impl fmt::Display for MountPointParsingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut msg = format!("Could not find a valid mountpoint for {}", self.mount_path);
        if let Some(inner) = &self.inner {
            msg += &*format!(": {}", inner);
        }
        write!(f, "{}", msg)
    }
}

impl std::error::Error for MountPointParsingError {

}

impl From<std::io::Error> for MountPointParsingError {
    fn from(e: std::io::Error) -> Self {
        MountPointParsingError{
            mount_path: "<unknown>".to_string(),
            inner: Option::from(e.to_string())
        }
    }
}

#[derive(Debug, Clone)]
struct BackupParsingError {
    message: String
}

impl fmt::Display for BackupParsingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error parsing backup: {}", self.message)
    }
}

impl std::error::Error for BackupParsingError { }

impl From<std::io::Error> for BackupParsingError {
    fn from(e: std::io::Error) -> Self {
        BackupParsingError{ message: format!("Received io::ERROR( \"{}\" )", e.to_string()) }
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