use std::collections::HashMap;

use egui::{emath::TSTransform, Ui};

use crate::{
    graphs::{add_main_node, add_node, get_connection, Ports, TransformClip},
    handle::Handle,
    http::{HttpContext, LogEntry},
};

use super::{State, Storage};

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, PartialEq)]
pub enum View {
    Graph,
    Text,
}

impl View {
    pub fn draw(
        &self,
        ui: &mut Ui,
        state: &mut State,
        storage: &mut Storage,
        http_ctx: &HttpContext,
    ) {
        match self {
            View::Graph => graph_view(ui, state, storage, http_ctx),
            View::Text => text_view(ui, state, storage, http_ctx),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            View::Graph => "Graph",
            View::Text => "Text",
        }
    }
}

pub fn graph_view(ui: &mut Ui, state: &mut State, storage: &mut Storage, http_ctx: &HttpContext) {
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

    let transform =
        TSTransform::from_translation(ui.min_rect().left_top().to_vec2()) * storage.transform;

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
            storage.transform = TSTransform::from_translation(pan_delta) * storage.transform;
        }
    }

    let clip = TransformClip { transform, rect };

    let mut handle_to_ports: HashMap<Handle, Ports> = HashMap::new();

    let main_handle = match Handle::from_hex(&state.target_input) {
        Ok(h) => {
            storage.target = h.clone();
            h
        }
        Err(e) => {
            state.error.write(format!("{:#}", e));
            storage.target.clone()
        }
    };

    handle_to_ports.insert(
        main_handle.clone(),
        add_main_node(
            http_ctx.clone(),
            main_handle.clone(),
            &state.connections,
            &mut state.target_input,
            state.error.read(),
            clip.clone(),
        ),
    );

    let painter = ui.painter();
    let painter = painter.with_clip_rect(rect);
    state.connections.visit_bfs(main_handle.clone(), {
        let connections = &state.connections;
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
}

pub fn text_view(ui: &mut Ui, state: &mut State, storage: &mut Storage, http_ctx: &HttpContext) {
    ui.heading("Bonjour  ");
    ui.separator();
    egui::ScrollArea::both()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.style_mut().wrap = Some(false);
            for (i, entry) in &state.log.log {
                match entry {
                    LogEntry::Request => {
                        ui.monospace(format!("[{i}]: Request"));
                    }
                    LogEntry::Response(c) => {
                        ui.monospace(format!("[{i}]: {} {}", c.lhs.to_hex(), c.rhs));
                    }
                }
            }
        });
}
