mod backups;

use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;
use std::sync::{Arc, Mutex, MutexGuard};
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use tide::log;
use std::env;
use failure::Error;
use std::fs::{File};
use std::io::BufReader;
use serde_json::{Value};
use crate::backups::BackupLabels;

#[async_std::main]
async fn main() -> std::result::Result<(), std::io::Error> {
    log::start();

    let mut registry = Registry::default();
    let backup_freshness = backups::get_backup_freshness();
    registry.register(
        "ncp_backup_freshness",
        "Age of the latest backup in minutes",
        backup_freshness.clone(),
    );

    let config_dir = env::var("NCP_CONFIG_DIR")
        .unwrap_or_else(|_| "/usr/local/etc".to_string());
    log::info!("Loading config from '{}/ncp-metrics.cfg'", config_dir);
    let file = File::open(format!("{}/ncp-metrics.cfg", config_dir))?;

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
            if let Err(e) = gather_metrics(&req.state().config.lock().unwrap(),
                           &req.state().metric_backup_freshness.lock().unwrap()) {

                log::error!("Error collecting metrics: {}", e)
            };

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

fn gather_metrics(config: &MutexGuard<Value>, backup_freshness: &MutexGuard<Family<BackupLabels, Gauge>>) -> Result<(), Error> {
    let bkp_config = config["backups"].as_array()
        .expect("Could not parse configuration: .[\"backups\"] needs to be an array");

    for bkp in bkp_config {
        let mount_path = bkp["path"].as_str()
            .expect("Could not parse configuration: .[\"backups\"][\"path\"] is missing");
        let bkp_pattern= bkp["pattern"].as_str()
            .expect("Could not parse configuration: .[\"backups\"][\"pattern\"] is missing");
        backups::measure_backup_freshness(mount_path, bkp_pattern, backup_freshness)?
    }
    Ok(())
}