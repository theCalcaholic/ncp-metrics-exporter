mod backups;

use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;
use std::sync::{Arc, Mutex, MutexGuard};
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use tide::{Middleware, Next, Request};
use tide::log;
use std::env;
use std::path::Iter;
use std::fs::{File, read};
use std::io::BufReader;
use shellwords::split;
use serde_json::{json, Result, Value};
use tide::http::url::OpaqueOrigin;
use crate::backups::BackupLabels;

#[async_std::main]
async fn main() -> std::result::Result<(), std::io::Error> {
    log::start();

    let mut registry = Registry::default();
    let backup_freshness = backups::get_backup_freshness();
    registry.register(
        "backup_freshness",
        "Age of the latest backup in minutes",
        backup_freshness.clone(),
    );

    log::info!("Loading config from '{}/ncp-metrics.cfg'",
                                  env::var("NCP_CONFIG_DIR")
                                      .unwrap_or(String::from("/usr/local/etc")));

    let file = File::open(format!("{}/ncp-metrics.cfg",
                                  env::var("NCP_CONFIG_DIR")
                                      .unwrap_or(String::from("/usr/local/etc")))).unwrap();
    let reader = BufReader::new(file);
    let config: Value = serde_json::from_reader(reader).unwrap();
    log::info!("Found: {:#?}", config);

    let mut app = tide::with_state(State {
        registry: Arc::new(Mutex::new(registry)),
        metric_backup_freshness: Arc::new(Mutex::new(backup_freshness)),
        config: Arc::new(Mutex::new(config))
    });

    app.at("/metrics")
        .get(|req: tide::Request<State>| async move {
            gather_metrics(&req.state().config.lock().unwrap(),
                           &req.state().metric_backup_freshness.lock().unwrap());
            let registry = &req.state().registry.lock().unwrap();
            let mut encoded = Vec::new();
            encode(&mut encoded, registry).unwrap();
            let response = tide::Response::builder(200)
                .body(encoded)
                //.content_type("application/openmetrics-text; version=1.0.0; charset=utf-8")
                .content_type("text/plain; version=1.0.0; charset=utf-8")
                .build();
            Ok(response)
        });

    app.listen("127.0.0.1:9000").await?;

    Ok(())
}

#[derive(Clone)]
struct State {
    registry: Arc<Mutex<Registry<Family<backups::BackupLabels, Gauge>>>>,
    metric_backup_freshness: Arc<Mutex<Family<backups::BackupLabels, Gauge>>>,
    config: Arc<Mutex<Value>>
}

fn gather_metrics(config: &MutexGuard<Value>, backup_freshness: &MutexGuard<Family<BackupLabels, Gauge>>) -> Option<()> {
    let bkp_default_pattern = json!(r".*_(?P<year>\d+)-(?P<month>\d+)-(?P<day>\d+)_(?P<hour>\d{2})(?P<minute>\d{2})(?P<second>\d{2})");

    let bkp_config = config["backups"].as_array().unwrap().iter().map(move |bkp_cfg| {
        (bkp_cfg["path"].as_str().unwrap(),
         bkp_cfg["pattern"].as_str().unwrap()) //.unwrap_or(&bkp_default_pattern.clone()).as_str().unwrap())
    });

    // let backup_paths_string = env::var("NCPMETRICS_BACKUP_PATHS")
    //     .unwrap_or("".to_string());
    //
    // log::info!("Received backup paths: {}", backup_paths_string);
    // let backup_paths = split(&backup_paths_string)
    //     .unwrap();
    //
    // let bkp_config = backup_paths.iter().map(|s| (s, bkp_name_pattern));
    // log::info!("Config: {:#?}", bkp_config);

    for (mount_path, bkp_pattern) in bkp_config {
        backups::measure_backup_freshness(mount_path, bkp_pattern, &backup_freshness);
    }
    Some(())
}