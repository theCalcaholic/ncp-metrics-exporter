mod backups;

use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;
use std::sync::{Arc, Mutex, MutexGuard};
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use tide::{Middleware, Next, Request, Result};
use std::env;
use std::path::Iter;
use shellwords::split;
use crate::backups::BackupLabels;

#[async_std::main]
async fn main() -> std::result::Result<(), std::io::Error> {
    tide::log::start();

    let mut registry = Registry::default();
    let backup_freshness = backups::get_backup_freshness();
    registry.register(
        "backup_freshness",
        "Age of the latest backup in minutes",
        backup_freshness.clone(),
    );

    // for (mount_path, bkp_pattern) in vec![("/snap/krita/64", r".*"), ("/snap/vlc/2344", r".*")] {
    //     backups::measure_backup_freshness(mount_path, bkp_pattern, &backup_freshness);
    // }
    //
    //
    // let mut buffer = vec![];
    // encode(&mut buffer, &registry).unwrap();
    // println!("{}", String::from_utf8(buffer).unwrap())


    let mut app = tide::with_state(State {
        registry: Arc::new(Mutex::new(registry)),
        metric_backup_freshness: Arc::new(Mutex::new(backup_freshness))
    });

    app.at("/metrics")
        .get(|req: tide::Request<State>| async move {
            gather_metrics(&req.state().metric_backup_freshness.lock().unwrap());
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
    metric_backup_freshness: Arc<Mutex<Family<backups::BackupLabels, Gauge>>>
}

fn gather_metrics(backup_freshness: &MutexGuard<Family<BackupLabels, Gauge>>) {

    let backup_paths_string = env::var("NCPMETRICS_BACKUP_PATHS")
        .unwrap_or("".to_string());
    let backup_paths = split(&backup_paths_string)
        .unwrap();
    let bkp_config = backup_paths.iter().map(|s| (s, r".*"));

    for (mount_path, bkp_pattern) in bkp_config {
        backups::measure_backup_freshness(mount_path, bkp_pattern, &backup_freshness);
    }
}