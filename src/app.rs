mod views;

use std::sync::{atomic::AtomicUsize, Arc};

use egui::{emath::TSTransform, Visuals};
use reqwest::Client;

use crate::{
    graphs::RelationStorage,
    handle::Handle,
    http::{HttpContext, HttpLog, LogEntry},
};

use self::views::View;

pub struct App {
    state: State,
    storage: Storage,
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
struct Storage {
    target: Handle,
    transform: TSTransform,
    view: View,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            target: Handle::from_hex(
                "1000000000000000000000000000000000000000000000000000000000000024",
            )
            .unwrap(),
            transform: TSTransform::default(),
            view: View::Graph,
        }
    }
}

struct State {
    target_input: String,
    error: Error,
    first_render: bool,
    client: Arc<Client>,
    log: HttpLog,
    connections: RelationStorage,
    counter: Arc<AtomicUsize>,
}

#[derive(Default)]
struct Error {
    content: String,
    dirty: bool,
}

impl Error {
    fn write(&mut self, content: String) {
        self.dirty = true;
        self.content = content;
    }

    fn read(&self) -> &str {
        if self.dirty {
            &self.content
        } else {
            ""
        }
    }

    fn clear(&mut self) {
        self.dirty = false;
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            target_input: "1000000000000000000000000000000000000000000000000000000000000024"
                .to_owned(),
            error: Error::default(),
            first_render: true,
            client: Arc::new(Client::new()),
            connections: RelationStorage::default(),
            counter: Arc::new(AtomicUsize::new(0)),
            log: HttpLog::new(),
        }
    }
}

impl App {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return Self {
                state: State::default(),
                storage: eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default(),
            };
        }

        cc.egui_ctx.set_visuals(Visuals::dark());

        App {
            state: State::default(),
            storage: Storage::default(),
        }
    }
}

impl eframe::App for App {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.storage);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        let storage = &mut self.storage;
        let State {
            error,
            client,
            connections,
            counter,
            log,
            ..
        } = &mut self.state;

        let http_ctx = HttpContext {
            client: client.clone(),
            egui_ctx: ctx.clone(),
            url_base: "127.0.0.1:9090".to_owned(),
            tx: log.tx.clone(),
            counter: counter.clone(),
        };

        if let Ok(new_connections) = log.rx.try_recv() {
            match new_connections {
                Ok(new_connections) => {
                    error.clear();
                    for (i, entry) in new_connections {
                        if let LogEntry::Response(c) = entry.clone() {
                            connections.insert(c.clone());
                        }
                        log.log.push((i, entry));
                    }
                }
                Err(e) => error.write(format!("{:#}", e)),
            }
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.visuals_mut().button_frame = false;
                ui.selectable_value(&mut storage.view, View::Graph, View::Graph.name());
                ui.selectable_value(&mut storage.view, View::Text, View::Text.name());
            });
        });

        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            // TODO add some nice controls
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            storage
                .view
                .clone()
                .draw(ui, &mut self.state, storage, &http_ctx);
            self.state.first_render = false;
        });
    }
}
