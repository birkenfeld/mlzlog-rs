use log::*;

fn main() {
    mlzlog::init(Some("log"), "testapp", mlzlog::Settings {
        debug: true,
        .. Default::default()
    }).unwrap();

    debug!("debug");
    info!("info");
    warn!("warn");
    error!("error");
}
