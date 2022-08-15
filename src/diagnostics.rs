use serde_json::{json, to_string, Value};
use std::fmt;
use std::fs::{File, read_to_string};
use std::io::Read;
use std::io::BufReader;
use std::process::Command;
use async_std::fs;
use regex::{Regex};
use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use nix::sys::utsname::uname;
use failure::{Error as FError};
use tide::StatusCode::Continue;


#[derive(Clone, Hash, PartialEq, Eq, Encode)]
pub(crate) struct DiagLabels {
    backups_disk: String,
    backups_path: String,
    backup_pattern: String
    //latest_backup_path: String
}

pub(crate) fn get_diagnostics_metric() -> Family<DiagLabels, Gauge> {
    Family::<DiagLabels, Gauge>::default()
}

pub(crate) fn run_diagnostics(metric: &Family<DiagLabels, Gauge>) -> Result<(), FError> {
    Ok(())
}

async fn get_ncp_version(config_path: &str) -> Option<String> {
    fs::read_to_string(format!("{}/ncp-baseimage", config_path)).await.ok()
}

async fn get_base_image(config_path: &str) -> Option<String> {
    fs::read_to_string(format!("{}/ncp-baseimage", config_path)).await.ok()
}

async fn get_os_info() -> Result<(String, String, String), String> {

    let uts_name = uname()
        .map_err(|e| format!("Error retrieving system information ({})", e.to_string()))?;
    let os_name = match fs::read_to_string("/etc/issue").await {
        Ok(s) => {
            let regex = Regex::new("\\\\.|\n").unwrap();
            regex.replace_all(&s, "").to_string()
        },
        Err(e) => uts_name.sysname().to_str().unwrap().to_string()
    };

    Ok((os_name,
        uts_name.release().to_str().unwrap().to_string(),
        uts_name.machine().to_str().unwrap().to_string()))

}

async fn get_automount(config_path: &str) -> Option<bool> {
    let am_config: Value = serde_json::from_str(
        &fs::read_to_string(format!("{}/ncp-config.d/nc-automount.cfg", config_path)).await.ok()?
    ).ok()?;
    Some(am_config["params"].as_array()?[0]["value"] == "true")
}

async fn get_usb_devices() -> Option<Vec<String>>{
    // TODO: Refactor
    // lsblk -S -o  NAME,TRAN | awk '{ if ( $2 == "usb" ) print $1; }' | tr '\n' ' '
    let stdout = Command::new("lsblk").args(vec!["-S", "-o", "NAME,TRAN"]).output().ok()?.stdout;
    let devs: Vec<String> = String::from_utf8_lossy(&*stdout).split("\n").filter_map(|line| {
        let cols: Vec<String> = line.split(" ").filter(|c| !c.is_empty()).map(|s| s.to_string()).collect();
        if cols.last().unwrap_or(&String::new()) == "usb" {
            Some(cols[0].clone())
        } else {
            None
        }
    }).collect();
    if devs.len() == 0 {
        None
    } else {
        Some(devs)
    }
}

async fn get_data_directory() -> Option<String> {
    let nc_config = fs::read_to_string("/var/www/nextcloud/config/config.php").await.ok()?;
    let regex = Regex::new("['\"]datadirectory['\"]\\s*=>\\s*['\"](?P<datadir>.*)['\"]").ok()?;
    Some(regex.captures(&*nc_config)?.name("datadir")?.as_str().to_string())
}


#[derive(Debug, Clone)]
struct DiagnosticsError {
    message: String
}

impl fmt::Display for DiagnosticsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error parsing backup: {}", self.message)
    }
}

impl std::error::Error for DiagnosticsError { }

impl From<std::io::Error> for DiagnosticsError {
    fn from(e: std::io::Error) -> Self {
        DiagnosticsError { message: format!("Received io::ERROR( \"{}\" )", e) }
    }
}


#[cfg(test)]
mod tests {
    use std::borrow::Borrow;
    use futures::executor::block_on;
    use super::*;

    #[async_std::test]
    async fn test_diagnostics() {
        let config_path = "/home/tobias/projects/nextcloudpi/etc";
        println!("Version: {}", get_ncp_version(config_path).await.unwrap_or("NONE".to_string()));
        println!("Image: {}", get_base_image(config_path).await.unwrap_or("NONE".to_string()));
        let (sysname, release, machine) = get_os_info().await.unwrap();
        println!("OS: {}. {} ({})", sysname, release, machine);
        println!("automount: {}", if get_automount(config_path).await.unwrap() { "yes" } else { "no" });
        let usb_devices = get_usb_devices().await.unwrap();
        println!("USB devices: {}", if usb_devices.is_empty() { "NONE".to_string() } else { usb_devices.join(" ") });
        println!("Data directory: {}", get_data_directory().await.unwrap_or("ERROR".to_string()))

    }
}
