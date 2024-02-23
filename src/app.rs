use std::{
    collections::HashMap,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
};

use anyhow::Result;
use egui::{Area, Id, LayerId, Vec2, Visuals};
use reqwest::Client;

use crate::{
    graphs::{add_main_node, add_node, get_connection, Graph, Ports, Relation},
    handle::Handle,
    http::HttpContext,
};

pub struct App {
    state: State,
    storage: Storage,
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
struct Storage {
    target: Handle,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            target: Handle::from_hex(
                "1000000000000000000000000000000000000000000000000000000000000024",
            )
            .unwrap(),
        }
    }
}

struct State {
    target_input: String,
    error: Error,
    first_render: bool,
    client: Arc<Client>,
    response_tx: Sender<Result<Vec<Relation>>>,
    response_rx: Receiver<Result<Vec<Relation>>>,
    connections: Graph,
    tick: usize,
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
        let (tx, rx) = channel();
        Self {
            target_input: "1000000000000000000000000000000000000000000000000000000000000024"
                .to_owned(),
            error: Error::default(),
            first_render: true,
            client: Arc::new(Client::new()),
            response_tx: tx,
            response_rx: rx,
            connections: Graph::default(),
            tick: 0,
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
            target_input,
            error,
            first_render,
            client,
            response_tx: tx,
            response_rx: rx,
            connections,
            tick,
        } = &mut self.state;

        let http_ctx = HttpContext {
            client: client.clone(),
            egui_ctx: ctx.clone(),
            url_base: "127.0.0.1:9090".to_owned(),
            tx: tx.clone(),
        };

        if let Ok(new_connections) = rx.try_recv() {
            match new_connections {
                Ok(new_connections) => {
                    error.clear();
                    for c in new_connections {
                        connections.insert(c);
                    }
                }
                Err(e) => error.write(format!("{:#}", e)),
            }
        }

        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            Area::new("bonjour")
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.heading("Bonjour Left");
                    let _ = ui.button("bonjour").clicked();
                    *tick += 1;

                    // TODO
                    // ctx.transform_layer(
                    //     LayerId::new(egui::Order::Foreground, Id::new("bonjour")),
                    //     Vec2::new(
                    //         50.0 + 10.0 * f32::cos(*tick as f32 / 10.0),
                    //         100.0 + 10.0 * f32::sin(*tick as f32 / 10.0),
                    //     ),
                    //     1.0 + f32::sin(*tick as f32 / 100.0),
                    // );
                })
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Objects");
            ui.separator();

            let mut handle_to_ports: HashMap<Handle, Ports> = HashMap::new();
            let painter = ui.painter();

            let main_handle = match Handle::from_hex(target_input) {
                Ok(h) => {
                    storage.target = h.clone();
                    h
                }
                Err(e) => {
                    error.write(format!("{:#}", e));
                    storage.target.clone()
                }
            };

            handle_to_ports.insert(
                main_handle.clone(),
                add_main_node(
                    http_ctx.clone(),
                    main_handle.clone(),
                    connections,
                    target_input,
                    error.read(),
                ),
            );

            connections.visit_bfs(main_handle.clone(), |connection| {
                let out_port = *handle_to_ports
                    .entry(connection.lhs.clone())
                    .or_insert_with(|| {
                        add_node(http_ctx.clone(), connection.lhs.clone(), connections)
                    })
                    .outputs
                    .get(&connection.relation_type)
                    .expect("Connection without port");
                let in_port = handle_to_ports
                    .entry(connection.rhs.clone())
                    .or_insert_with(|| {
                        add_node(http_ctx.clone(), connection.rhs.clone(), connections)
                    })
                    .input;

                painter.add(get_connection(
                    out_port,
                    in_port,
                    connection.relation_type,
                    connection.lhs == connection.rhs,
                ));
            });
        });
        // // ctx.set_embed_viewports(true);
        // ctx.show_viewport_deferred(
        //     egui::ViewportId::from_hash_of("panels_viewport"),
        //     ViewportBuilder::default(),
        //     |ctx, class| {
        //         // ctx.set_zoom_factor(0.5);
        //         egui::CentralPanel::default().show(ctx, |ui| {
        //             ui.heading("bonk");
        //         });
        //     },
        // );

        *first_render = false;
    }
}
