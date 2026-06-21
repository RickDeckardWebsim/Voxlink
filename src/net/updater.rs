use std::sync::mpsc;
use std::thread;

pub enum UpdaterEvent {
    UpdateAvailable(String),
    UpdateFinished,
    UpdateFailed(String),
}

pub fn check_for_updates(tx: mpsc::Sender<UpdaterEvent>) {
    thread::spawn(move || {
        let latest_release = match self_update::backends::github::Update::configure()
            .repo_owner("RickDeckardWebsim")
            .repo_name("Voxlink")
            .bin_name("voxlink")
            .current_version(env!("CARGO_PKG_VERSION"))
            .build()
        {
            Ok(updater) => updater.get_latest_release(),
            Err(e) => {
                log::error!("Failed to configure updater: {}", e);
                return;
            }
        };

        let release = match latest_release {
            Ok(r) => r,
            Err(e) => {
                log::error!("Failed to fetch latest release info: {}", e);
                return;
            }
        };

        if let Ok(is_greater) =
            self_update::version::bump_is_greater(env!("CARGO_PKG_VERSION"), &release.version)
        {
            if is_greater {
                let _ = tx.send(UpdaterEvent::UpdateAvailable(release.version));
            }
        }
    });
}

pub fn run_update(tx: mpsc::Sender<UpdaterEvent>) {
    thread::spawn(move || {
        let update_result = self_update::backends::github::Update::configure()
            .repo_owner("RickDeckardWebsim")
            .repo_name("Voxlink")
            .bin_name("voxlink")
            .show_download_progress(true)
            .current_version(env!("CARGO_PKG_VERSION"))
            .build()
            .and_then(|updater| updater.update());

        match update_result {
            Ok(_) => {
                let _ = tx.send(UpdaterEvent::UpdateFinished);
                
                // Automatically restart the application!
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).spawn();
                    std::process::exit(0);
                }
            }
            Err(e) => {
                let _ = tx.send(UpdaterEvent::UpdateFailed(e.to_string()));
            }
        }
    });
}
