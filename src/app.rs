use std::{
    collections::HashMap,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
};

use anyhow::Result;
use egui::{emath::TSTransform, Visuals};
use reqwest::Client;

use crate::{
    graphs::{
        add_main_node, add_node, get_connection, Ports, Relation, RelationStorage, TransformClip,
    },
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
    transform: TSTransform,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            target: Handle::from_hex(
                "1000000000000000000000000000000000000000000000000000000000000024",
            )
            .unwrap(),
            transform: TSTransform::default(),
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
    connections: RelationStorage,
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
            connections: RelationStorage::default(),
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
            // TODO add some nice controls
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Objects");
            ui.separator();

            let (id, rect) = ui.allocate_space(ui.available_size());
            let response = ui.interact(rect, id, egui::Sense::click_and_drag());
            // Allow dragging the background as well.
            if response.dragged() {
                storage.transform.translation += response.drag_delta();
            }

            // Plot-like reset
            if response.double_clicked() {
                storage.transform = TSTransform::default();
            }

            let transform = TSTransform::from_translation(ui.min_rect().left_top().to_vec2())
                * storage.transform;

            if let Some(pointer) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                // Note: doesn't catch zooming / panning if a button in this PanZoom container is hovered.
                if response.hovered() {
                    let pointer_in_layer = transform.inverse() * pointer;
                    let zoom_delta = ui.ctx().input(|i| i.zoom_delta());
                    let pan_delta = ui.ctx().input(|i| i.smooth_scroll_delta);

                    // Zoom in on pointer:
                    storage.transform = storage.transform
                        * TSTransform::from_translation(pointer_in_layer.to_vec2())
                        * TSTransform::from_scaling(zoom_delta)
                        * TSTransform::from_translation(-pointer_in_layer.to_vec2());

                    // Pan:
                    storage.transform =
                        TSTransform::from_translation(pan_delta) * storage.transform;
                }
            }

            let clip = TransformClip { transform, rect };

            let mut handle_to_ports: HashMap<Handle, Ports> = HashMap::new();

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
                    clip.clone(),
                ),
            );

            let painter = ui.painter();
            connections.visit_bfs(main_handle.clone(), {
                let connections = &connections;
                move |connection| {
                    if let Some((port_type, rhs)) = connection.rhs.get_port_type() {
                        let out_port = *handle_to_ports
                            .entry(connection.lhs.clone())
                            .or_insert_with({
                                || {
                                    add_node(
                                        http_ctx.clone(),
                                        connection.lhs.clone(),
                                        connections,
                                        clip.clone(),
                                    )
                                }
                            })
                            .outputs
                            .get(&port_type)
                            .expect("Connection without port");
                        let in_port = handle_to_ports
                            .entry(rhs.clone())
                            .or_insert_with({
                                let clip = clip.clone();
                                || add_node(http_ctx.clone(), rhs.clone(), connections, clip)
                            })
                            .input;
                        let clip = clip.clone();
                        // TODO: clip bezier
                        painter.add(get_connection(
                            out_port,
                            in_port,
                            port_type,
                            connection.lhs == rhs,
                            clip,
                        ));
                    }
                }
            });
        });

        *first_render = false;
    }
}
