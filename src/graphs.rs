use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fmt::Display,
};

use egui::{
    emath::TSTransform, epaint::CubicBezierShape, Color32, Grid, Id, InnerResponse, Label, Layout,
    Margin, Pos2, Rect, RichText, Sense, Stroke, TextStyle, Ui, Vec2,
};

use crate::{
    handle::{Handle, Operation},
    http::{HttpContext, Request},
};

/// Stores all the information we have obtained from the API.
#[derive(Default)]
pub(crate) struct RelationStorage {
    forward: HashMap<Handle, BTreeSet<Relation>>,
    backward: HashMap<Handle, BTreeSet<Relation>>,
}

impl RelationStorage {
    pub(crate) fn insert(&mut self, relation: Relation) {
        self.forward
            .entry(relation.lhs.clone())
            .or_default()
            .insert(relation.clone());
        match &relation.rhs {
            RelationRhs::Eval(h)
            | RelationRhs::Apply(h)
            | RelationRhs::Pin(h)
            | RelationRhs::TagAuthor(h)
            | RelationRhs::TagTarget(h)
            | RelationRhs::TagLabel(h)
            | RelationRhs::TreeEntry(h, _) => {
                self.backward.entry(h.clone()).or_default().insert(relation);
            }
            RelationRhs::Description(_) => {}
        }
    }

    pub(crate) fn visit_bfs(&self, root: Handle, mut handle: impl FnMut(&Relation)) {
        fn handle_relations(
            relations: &BTreeSet<Relation>,
            to_visit: &mut VecDeque<Handle>,
            seen: &mut HashSet<Handle>,
            handle: &mut impl FnMut(&Relation),
            selector: impl Fn(&Relation) -> Option<Handle>,
        ) {
            for relation in relations {
                let target = selector(relation);
                if let Some(h) = target {
                    if !seen.contains(&h) {
                        handle(relation);
                        to_visit.push_back(h.clone());
                        seen.insert(h.clone());
                    }
                }
            }
        }
        let mut seen = HashSet::new();
        let mut to_visit: VecDeque<_> = vec![root].into();
        while let Some(next) = to_visit.pop_front() {
            if let Some(relations) = self.forward.get(&next) {
                handle_relations(relations, &mut to_visit, &mut seen, &mut handle, |r| {
                    r.rhs.get_port_type().map(|(_, h)| h)
                })
            }
            if let Some(relations) = self.backward.get(&next) {
                handle_relations(relations, &mut to_visit, &mut seen, &mut handle, |r| {
                    Some(r.lhs.clone())
                })
            }
        }
    }
}

/// Information related to Handles that we have obtained from the API.
#[derive(Hash, PartialEq, Eq, Clone, Debug, PartialOrd, Ord)]
pub(crate) struct Relation {
    pub(crate) lhs: Handle,
    pub(crate) rhs: RelationRhs,
}

impl Relation {
    pub(crate) fn new(lhs: Handle, rhs: RelationRhs) -> Self {
        Self { lhs, rhs }
    }
}

// For now. Should add content, tag.
/// The order of these fields dictates the order in which they show up in the
/// visualization windows.
#[derive(Hash, PartialEq, Eq, Clone, Debug, PartialOrd, Ord)]
pub enum RelationRhs {
    Eval(Handle),
    Apply(Handle),
    Pin(Handle),
    TagAuthor(Handle),
    TagTarget(Handle),
    TagLabel(Handle),
    TreeEntry(Handle, usize),
    Description(String),
}

impl RelationRhs {
    fn get_abbrev(&self) -> Cow<str> {
        match self {
            Self::Eval(_) => Cow::Borrowed("evaluates into"),
            Self::Apply(_) => Cow::Borrowed("applies into"),
            Self::Pin(_) => Cow::Borrowed("pins"),
            Self::TagAuthor(_) => Cow::Borrowed("this object"),
            Self::TagTarget(_) => Cow::Borrowed("tags this object"),
            Self::TagLabel(_) => Cow::Borrowed("with this label"),
            Self::TreeEntry(_, i) => Cow::Owned(format!("has entry at index [{}]", i)),
            Self::Description(s) => Cow::Borrowed(s.as_str()),
        }
    }
}

impl Display for RelationRhs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eval(h) => f.write_fmt(format_args!("evaluates into {}", h.to_hex())),
            Self::Apply(h) => f.write_fmt(format_args!("applies into {}", h.to_hex())),
            Self::Pin(h) => f.write_fmt(format_args!("pins {}", h.to_hex())),
            Self::TagAuthor(h) => f.write_fmt(format_args!("this object {}", h.to_hex())),
            Self::TagTarget(h) => f.write_fmt(format_args!("tags this object {}", h.to_hex())),
            Self::TagLabel(h) => f.write_fmt(format_args!("with this label {}", h.to_hex())),
            Self::TreeEntry(h, i) => {
                f.write_fmt(format_args!("has entry {} at index [{}]", h.to_hex(), i))
            }
            Self::Description(s) => f.write_fmt(format_args!("{}", s.as_str())),
        }
    }
}

impl RelationRhs {
    pub fn get_port_type(&self) -> Option<(PortType, Handle)> {
        match self.clone() {
            RelationRhs::Eval(h) => Some((PortType::Eval, h)),
            RelationRhs::Apply(h) => Some((PortType::Apply, h)),
            RelationRhs::Pin(h) => Some((PortType::Pin, h)),
            RelationRhs::TagAuthor(h) => Some((PortType::TagAuthor, h)),
            RelationRhs::TagTarget(h) => Some((PortType::TagTarget, h)),
            RelationRhs::TagLabel(h) => Some((PortType::TagLabel, h)),
            RelationRhs::TreeEntry(h, i) => Some((PortType::TreeEntry(i), h)),
            RelationRhs::Description(_) => None,
        }
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub enum PortType {
    Eval,
    Apply,
    Pin,
    TagAuthor,
    TagTarget,
    TagLabel,
    TreeEntry(usize),
}

impl PortType {
    fn get_color(&self) -> Color32 {
        match self {
            Self::Eval => Color32::BLUE,
            Self::Apply => Color32::GREEN,
            Self::Pin => Color32::GRAY,
            Self::TagTarget => Color32::LIGHT_BLUE,
            Self::TagAuthor => Color32::LIGHT_GREEN,
            Self::TagLabel => Color32::LIGHT_RED,
            Self::TreeEntry(_) => Color32::GRAY,
        }
    }
}

pub(crate) struct Ports {
    pub input: Pos2,
    pub outputs: HashMap<PortType, Pos2>,
}

#[derive(Clone)]
pub(crate) struct TransformClip {
    pub transform: TSTransform,
    pub rect: Rect,
}

fn add_object(
    ctx: &egui::Context,
    window_id: impl std::hash::Hash,
    handle: Handle,
    start_pos: Pos2,
    forward_relations: Option<&BTreeSet<Relation>>,
    add_contents: impl FnOnce(&mut Ui) -> f32,
    clip: TransformClip,
) -> Ports {
    fn add_dot(ui: &mut Ui, center: Pos2) {
        ui.allocate_rect(
            Rect::from_center_size(center, Vec2::new(10.0, 10.0)),
            Sense::click_and_drag(),
        );
        ui.painter()
            .circle(center, 4.0, Color32::WHITE, Stroke::NONE);
    }

    fn main_body<'a>(
        ui: &mut Ui,
        handle: Handle,
        add_contents: impl FnOnce(&mut Ui) -> f32,
        forward_relations: Option<&'a BTreeSet<Relation>>,
    ) -> HashMap<PortType, f32> {
        ui.add(Label::new(
            // TODO handle more information
            RichText::new(format!("{}", handle.to_hex()))
                .text_style(TextStyle::Button)
                .color(ui.style().visuals.strong_text_color()),
        ));
        add_contents(ui);
        ui.separator();
        let mut ports = HashMap::new();
        if let Some(relations) = forward_relations {
            for relation in relations {
                // Sorted by relation type.
                let start_height = ui.min_rect().bottom();
                ui.label(relation.rhs.get_abbrev());
                let end_height = ui.min_rect().bottom();
                ui.end_row();
                if let Some((port_type, _)) = relation.rhs.get_port_type() {
                    ports.insert(port_type, (start_height + end_height) / 2.0);
                }
            }
        }
        ports
    }

    // Note that the window_id should not be derived from the handle.
    // This allows the "main" window with an editable handle to not
    // jump around while the user types into it.

    let v = egui::containers::Area::new(Id::new(window_id))
        .default_pos(start_pos)
        .movable(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_clip_rect(clip.transform.inverse() * clip.rect);
            ui.with_layout(Layout::default().with_main_wrap(false), |ui| {
                // ui.style_mut().wrap = Some(false);
                let InnerResponse { inner, response } = egui::Frame::default()
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(Margin::same(8.0))
                    .stroke(ctx.style().visuals.window_stroke)
                    .fill(ui.style().visuals.panel_fill)
                    .show(ui, |ui| {
                        egui::containers::Resize::default()
                            .id((handle.to_hex() + " resizable window").into())
                            .with_stroke(false)
                            .show(ui, |ui| {
                                main_body(ui, handle, add_contents, forward_relations)
                            })
                    });
                let window_center = response.rect.center().y;
                let dot_center = Pos2::new(ui.min_rect().left(), window_center);
                add_dot(ui, dot_center);

                let outputs: HashMap<_, _> = inner
                    .into_iter()
                    .map(|(r_type, height)| (r_type, Pos2::new(ui.min_rect().right(), height)))
                    .collect();
                for pos in outputs.values() {
                    add_dot(ui, *pos);
                }

                Ports {
                    input: dot_center,
                    outputs,
                }
            })
            .inner
        });

    ctx.set_transform_layer(v.response.layer_id, clip.transform);
    v.inner
}

fn add_fetch_buttons(ui: &mut Ui, ctx: HttpContext, handle: &Handle) {
    if ui.button("get description").clicked() {
        Request::Description(handle.clone()).send(ctx.clone());
    }

    if ui.button("eval").clicked() {
        Request::Relations(handle.clone(), Operation::Eval).send(ctx.clone());
    }
    if ui.button("apply").clicked() {
        Request::Relations(handle.clone(), Operation::Apply).send(ctx.clone());
    }

    // // TODO: add blob, and maybe thunk pointing to tree.
    if ui.button("get contents").clicked() {
        // http::get_contents(ctx.clone(), handle);
        // match handle.get_content_type() {
        //     Object::Tree => http::get_tree_contents(ctx.clone(), handle),
        //     Object::Tag => http::get_tag_contents(ctx.clone(), handle, None),
        //     _ => unreachable!(),
        // }
        Request::Contents(handle.clone()).send(ctx.clone());
    }

    if ui.button("get explanations").clicked() {
        Request::Explanations(handle.clone()).send(ctx.clone());
    }
    // ui.end_row();
}

pub(crate) fn add_main_node(
    ctx: HttpContext,
    handle: Handle,
    graph: &RelationStorage,
    target_input: &mut String,
    error: &str,
    clip: TransformClip,
) -> Ports {
    add_object(
        &ctx.egui_ctx,
        "main object",
        handle.clone(),
        Pos2::new(20.0, 20.0),
        graph.forward.get(&handle),
        |ui| {
            let middle_height = Grid::new(handle.to_hex() + " properties")
                .num_columns(2)
                .show(ui, |ui| {
                    let start_y = ui.min_rect().bottom();
                    ui.label("Handle:");
                    ui.text_edit_singleline(target_input);
                    ui.end_row();
                    ui.label("Error: ");
                    ui.label(error);
                    ui.end_row();

                    (ui.min_rect().bottom() + start_y) / 2.0
                })
                .inner;

            add_fetch_buttons(ui, ctx.clone(), &handle);

            middle_height
        },
        clip,
    )
}

pub(crate) fn add_node(
    ctx: HttpContext,
    handle: Handle,
    graph: &RelationStorage,
    clip: TransformClip,
) -> Ports {
    add_object(
        &ctx.egui_ctx,
        handle.clone(),
        handle.clone(),
        Pos2::new(20.0, 20.0),
        graph.forward.get(&handle),
        |ui| {
            let middle_height = Grid::new(handle.to_hex() + " properties")
                .num_columns(2)
                .show(ui, |ui| {
                    let start_y = ui.min_rect().bottom();
                    ui.label("Handle:");

                    let label = Label::new(handle.to_hex())
                        .truncate(true)
                        .sense(Sense::click());
                    if ui
                        .add(label)
                        .on_hover_cursor(egui::CursorIcon::Copy)
                        .clicked()
                    {
                        ui.output_mut(|o| o.copied_text = handle.to_hex())
                    };
                    ui.end_row();

                    (ui.min_rect().bottom() + start_y) / 2.0
                })
                .inner;

            add_fetch_buttons(ui, ctx.clone(), &handle);

            middle_height
        },
        clip,
    )
}

fn get_bezier(
    src: Pos2,
    src_dir: Vec2,
    dst: Pos2,
    dst_dir: Vec2,
    color: Color32,
    clip: TransformClip,
) -> CubicBezierShape {
    let connection_stroke = egui::Stroke {
        width: 5.0 * clip.transform.scaling,
        color,
    };

    let x_dist = dst.x - src.x;
    let control_scale = (x_dist / 2.0).max(-x_dist / 4.0).max(80.0);
    let src_control = src + src_dir * control_scale;
    let dst_control = dst + dst_dir * control_scale;

    CubicBezierShape::from_points_stroke(
        [src, src_control, dst_control, dst].map(|p| clip.transform.mul_pos(p)),
        false,
        Color32::TRANSPARENT,
        connection_stroke,
    )
}

pub(crate) fn get_connection(
    src: Pos2,
    dst: Pos2,
    port_type: PortType,
    is_self_loop: bool,
    clip: TransformClip,
) -> CubicBezierShape {
    let (src_dir, dst_dir) = if is_self_loop {
        (5.0 * (Vec2::X + Vec2::Y), -5.0 * (Vec2::X + Vec2::Y))
    } else {
        (Vec2::X, -Vec2::X)
    };
    get_bezier(src, src_dir, dst, dst_dir, port_type.get_color(), clip)
}
