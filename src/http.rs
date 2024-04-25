use std::sync::{
    atomic::{AtomicU32, AtomicUsize, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};

use anyhow::{ensure, Context, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::{
    graphs::{Relation, RelationRhs},
    handle::{Handle, Operation},
};

#[derive(Clone)]
pub(crate) struct HttpContext {
    pub(crate) client: Arc<Client>,
    pub(crate) egui_ctx: egui::Context,
    pub(crate) url_base: String,
    pub(crate) tx: Sender<Result<Vec<(usize, LogEntry)>>>,
    pub(crate) counter: Arc<AtomicUsize>,
}

pub(crate) struct HttpLog {
    pub tx: Sender<Result<Vec<(usize, LogEntry)>>>,
    pub rx: Receiver<Result<Vec<(usize, LogEntry)>>>,
    pub log: Vec<(usize, LogEntry)>,
}

#[derive(Clone)]
pub(crate) enum LogEntry {
    // TODO: have enum for request types
    Request,
    Response(Relation),
}

impl HttpLog {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        HttpLog {
            tx,
            rx,
            log: vec![],
        }
    }
}

pub(crate) fn get<T, F>(ctx: HttpContext, map: F, url_path: String)
where
    T: DeserializeOwned + Send,
    F: FnOnce(T) -> Result<Vec<Relation>> + Send + 'static,
{
    let count = ctx.counter.fetch_add(1, Ordering::SeqCst);
    let _ = ctx.tx.send(Ok(vec![(count, LogEntry::Request)]));
    let task = async move {
        let result = ctx
            .client
            .get(format!("http://{}{}", ctx.url_base, url_path))
            .send()
            .await;
        match result {
            Ok(ok) => {
                let json = ok.json::<T>().await;
                let _ = ctx
                    .tx
                    .send(json.context("parsing json").and_then(map).map(|v| {
                        v.into_iter()
                            .map(|r| (count, LogEntry::Response(r)))
                            .collect()
                    }));
            }
            Err(e) => {
                let _ = ctx
                    .tx
                    .send(Err(anyhow::anyhow!(format!("request failed: {}", e))));
            }
        }
        ctx.egui_ctx.request_repaint();
    };
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(task);
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::let_underscore_future)]
    let _ = tokio::spawn(task);
}

#[derive(serde::Deserialize)]
struct JsonRelation {
    op: String,
    lhs: String,
    rhs: String,
}

// The specific Boost for C++ being used only support property trees
// which serialize empty arrays as the empty string.
// Therefore, we catch the different type with this enum.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum EmptyStringOrVec<T> {
    String(String),
    Vec(Vec<T>),
}

fn parse_handle(handle: impl AsRef<str>) -> Result<Handle> {
    Handle::from_hex(handle.as_ref()).with_context(|| format!("parsing {}", handle.as_ref()))
}

pub(crate) fn get_explanations(ctx: HttpContext, handle: &Handle) {
    #[derive(serde::Deserialize)]
    struct JsonResponse {
        target: String,
        relations: EmptyStringOrVec<JsonRelation>,
        handles: EmptyStringOrVec<String>,
    }
    let handle_clone = handle.clone();

    get(
        ctx.clone(),
        move |json: JsonResponse| {
            let mut results = vec![];
            if let EmptyStringOrVec::Vec(handles) = json.handles {
                let handles = handles
                    .iter()
                    .map(parse_handle)
                    .collect::<Result<Vec<_>>>()?;
                get_pins_and_tags(ctx.clone(), &handle_clone, handles)
            }
            if let EmptyStringOrVec::Vec(relations) = json.relations {
                for relation in relations {
                    let rhs = parse_handle(&relation.rhs)?;
                    let result = Relation {
                        lhs: parse_handle(&relation.lhs)?,
                        rhs: match relation
                            .op
                            .parse::<u8>()
                            .context("parsing op")
                            .and_then(|i| TryInto::<Operation>::try_into(i))
                            .context("parsing op")?
                        {
                            Operation::Eval => RelationRhs::Eval(rhs),
                            Operation::Apply => RelationRhs::Apply(rhs),
                        },
                    };
                    results.push(result)
                }
            }

            Ok(results)
        },
        format!("/explanations?handle={}", handle.to_hex()),
    );
}

pub(crate) fn get_pins_and_tags(ctx: HttpContext, target: &Handle, pins_and_tags: Vec<Handle>) {
    // For every handle, if it is a tag, then fetch it.
    // If the target handle is in the the first slot of the tag, then
    // we use add a tag relation from the tag to the target.
    // Otherwise, we add a pin relation from the handle to the target.

    for handle in pins_and_tags {
        let target = target.clone();
        todo!()
        // if handle.get_content_type() == Object::Tag {
        //     get_tag_contents(ctx.clone(), &handle.clone(), Some(target.clone()));
        // } else {
        //     let _ = ctx.tx.send(Ok(vec![Relation::new(
        //         target.clone(),
        //         handle,
        //         RelationType::Pin,
        //     )]));
        // }
    }
}

// pub(crate) fn get_tag_contents(ctx: HttpContext, handle: &Handle, with_pin_target: Option<Handle>) {
//     #[derive(serde::Deserialize)]
//     struct JsonResponse {
//         handles: EmptyStringOrVec<String>,
//     }

//     let handle_c = handle.clone();

//     get(
//         ctx.clone(),
//         move |json: JsonResponse| {
//             let EmptyStringOrVec::Vec(handles) = json.handles else {
//                 return Err(anyhow!("expected tag to have children"));
//             };
//             let handles = handles
//                 .into_iter()
//                 .map(parse_handle)
//                 .collect::<Result<Vec<_>>>()?;
//             let handles: [Handle; 3] = handles
//                 .try_into()
//                 .map_err(|e: Vec<_>| anyhow!("found {} handles", e.len()))
//                 .context("expected tag to have three children")?;

//             let mut result = vec![
//                 Relation::new(
//                     handle_c.clone(),
//                     handles[0].clone(),
//                     RelationType::TagTarget,
//                 ),
//                 Relation::new(
//                     handle_c.clone(),
//                     handles[1].clone(),
//                     RelationType::TagAuthor,
//                 ),
//                 Relation::new(handle_c.clone(), handles[2].clone(), RelationType::TagLabel),
//             ];
//             if let Some(target) = with_pin_target {
//                 result.push(Relation::new(handle_c.clone(), target, RelationType::Pin))
//             }

//             Ok(result)
//         },
//         format!("/tree_contents?handle={}", handle.to_hex()),
//     )
// }

pub(crate) fn get_tree_contents(ctx: HttpContext, handle: &Handle) {
    #[derive(serde::Deserialize)]
    struct JsonResponse {
        handles: EmptyStringOrVec<String>,
    }

    let handle_clone = handle.clone();

    get(
        ctx.clone(),
        move |json: JsonResponse| {
            let EmptyStringOrVec::Vec(entries) = json.handles else {
                return Ok(vec![]);
            };
            let entries = entries
                .into_iter()
                .map(parse_handle)
                .collect::<Result<Vec<_>>>()?;
            Ok(entries
                .into_iter()
                .enumerate()
                .map(|(i, e)| Relation::new(handle_clone.clone(), RelationRhs::TreeEntry(e, i)))
                .collect())
        },
        format!("/tree_contents?handle={}", handle.to_hex()),
    );
}

pub(crate) fn get_description(ctx: HttpContext, handle: &Handle) {
    #[derive(serde::Deserialize)]
    struct JsonResponse {
        description: String,
    }
    let hex = handle.to_hex();
    let handle = handle.clone();

    get(
        ctx.clone(),
        move |json: JsonResponse| {
            Ok(vec![Relation::new(
                handle.clone(),
                RelationRhs::Description(json.description),
            )])
        },
        format!("/description?handle={}", hex),
    );
}

pub(crate) fn get_relation(ctx: HttpContext, handle: &Handle, op: Operation) {
    let hex = handle.to_hex();
    let handle = handle.clone();

    get(
        ctx.clone(),
        move |json: JsonRelation| {
            let fetched_op = json
                .op
                .parse::<u8>()
                .context("parsing op")
                .and_then(|i| TryInto::<Operation>::try_into(i))
                .context("parsing op")?;
            ensure!(fetched_op == op, "got different op back than requested");
            Ok(vec![Relation::new(
                handle.clone(),
                match op {
                    Operation::Apply => RelationRhs::Apply,
                    Operation::Eval => RelationRhs::Eval,
                }(Handle::from_hex(&json.rhs).context("parsing handle")?),
            )])
        },
        format!("/relation?handle={}&op={}", hex, op as u8),
    );
}

// pub(crate) fn get_contents(ctx: HttpContext, handle: &Handle) {
//     // TODO handle blob, tag in one response
//     #[derive(serde::Deserialize)]
//     struct JsonResponse {
//         handles: EmptyStringOrVec<String>,
//     }

//     let handle_clone = handle.clone();

//     get(
//         ctx.clone(),
//         move |json: JsonResponse| {
//             let EmptyStringOrVec::Vec(entries) = json.handles else {
//                 return Ok(vec![]);
//             };
//             let entries = entries
//                 .into_iter()
//                 .map(parse_handle)
//                 .collect::<Result<Vec<_>>>()?;
//             Ok(entries
//                 .into_iter()
//                 .enumerate()
//                 .map(|(i, e)| Relation::new(handle_clone.clone(), e, RelationType::TreeEntry(i)))
//                 .collect())
//         },
//         format!("/contents?handle={}", handle.to_hex()),
//     );
// }
