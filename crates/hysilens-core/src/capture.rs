use crate::{
    model::Playback,
    platforms::{AdapterState, CatalogUpdate, Context, Registration},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

pub enum Event {
    Playback(Playback, Vec<AdapterState>),
    Catalog(CatalogUpdate),
}

pub fn spawn(
    origin: Instant,
    registrations: Vec<Registration>,
    stop: Arc<AtomicBool>,
) -> mpsc::Receiver<Event> {
    let (sender, receiver) = mpsc::channel(32);

    let (catalogs, mut updates) = mpsc::channel(32);

    let catalog_events = sender.clone();

    tokio::spawn(async move {
        while let Some(update) = updates.recv().await {
            if catalog_events.send(Event::Catalog(update)).await.is_err() {
                break;
            }
        }
    });

    std::thread::spawn(move || {
        let mut adapters: Vec<_> = registrations
            .into_iter()
            .map(|registration| {
                let adapter = (registration.create)(Context {
                    origin,
                    catalogs: catalogs.clone(),
                    stop: stop.clone(),
                });

                (registration.descriptor, adapter)
            })
            .collect();

        while !stop.load(Ordering::Relaxed) {
            let mut playback = Playback::default();

            let mut states = Vec::new();

            let mut errors = Vec::new();

            for (descriptor, adapter) in &mut adapters {
                let sample = adapter.sample();

                playback.sessions.extend(sample.playback.sessions);

                if let Some(error) = sample.playback.error {
                    errors.push(format!("{}: {error}", descriptor.session_id));
                }

                states.push(AdapterState {
                    descriptor: *descriptor,
                    document: sample.document,
                });
            }

            playback.server_ms = origin.elapsed().as_secs_f64() * 1000.0;

            playback.error = (!errors.is_empty()).then(|| errors.join("; "));

            if sender
                .blocking_send(Event::Playback(playback, states))
                .is_err()
            {
                break;
            }

            std::thread::sleep(Duration::from_millis(250));
        }
    });

    receiver
}
