use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};

use anyhow::{bail, ensure, Context, Result};
use reqwest::Client;

use crate::{
    graphs::{Relation, RelationRhs},
    handle::{Handle, Operation},
};

#[derive(Clone)]
pub(crate) struct HttpContext {
    pub client: Arc<Client>,
    pub egui_ctx: egui::Context,
    pub url_base: String,
    pub tx: Sender<Result<Vec<(usize, LogEntry)>>>,
    pub counter: Arc<AtomicUsize>,
}

pub(crate) struct HttpLog {
    pub tx: Sender<Result<Vec<(usize, LogEntry)>>>,
    pub rx: Receiver<Result<Vec<(usize, LogEntry)>>>,
    pub log: Vec<(usize, LogEntry)>,
    pub command_input: String,
}

#[derive(Clone)]
pub(crate) enum LogEntry {
    // TODO: have enum for request types
    Request(String),
    Response(Relation),
}

#[derive(Clone)]
pub(crate) enum Request {
    Explanations(Handle),
    Contents(Handle),
    Description(Handle),
    Relations(Handle, Operation),
}

impl Request {
    pub(crate) fn to_cli(&self) -> String {
        match self {
            Request::Explanations(h) => format!("explanations {}", h.to_hex()),
            Request::Contents(h) => format!("contents {}", h.to_hex()),
            Request::Description(h) => format!("description {}", h.to_hex()),
            Request::Relations(h, o) => format!("relations {} {}", h.to_hex(), o),
        }
    }

    // TODO: check for help and print message first.
    pub(crate) fn from_cli(str: &str) -> Result<Self> {
        let mut args = str.split_whitespace();
        let Some(first) = args.next() else {
            bail!("Missing argument at position 0. See help.");
        };
        fn take_handle<'a>(
            args: &mut impl Iterator<Item = &'a str>,
            position: usize,
        ) -> Result<Handle> {
            args.next()
                .with_context(|| format!("Expected handle argument at position {position}"))
                .and_then(|h| {
                    Handle::from_hex(h).with_context(|| format!("at position {position}"))
                })
        }

        match first {
            "explanations" => take_handle(&mut args, 1).map(Request::Explanations),
            "contents" => take_handle(&mut args, 1).map(Request::Contents),
            "description" => take_handle(&mut args, 1).map(Request::Description),
            "relations" => take_handle(&mut args, 1).and_then(|h| {
                Ok(Request::Relations(
                    h,
                    args.next()
                        .context("Expected op argument at position 2")
                        .and_then(|o| o.parse::<Operation>().context("at position 2"))?,
                ))
            }),
            _ => bail!("Invalid argument at position 0 {}", first),
        }
    }

    fn to_url_path(&self) -> String {
        match self {
            Request::Explanations(h) => format!("/explanations?handle={}", h.to_hex()),
            // TODO: handle other types of content
            Request::Contents(h) => format!("/tree_contents?handle={}", h.to_hex()),
            Request::Description(h) => format!("/description?handle={}", h.to_hex()),
            Request::Relations(h, o) => format!("/relation?handle={}&op={}", h.to_hex(), *o as u8),
        }
    }

    async fn parse(&self, response: reqwest::Response) -> Result<Vec<Relation>> {
        async fn to_json<T: for<'a> serde::Deserialize<'a>>(
            response: reqwest::Response,
        ) -> Result<T> {
            response.json::<T>().await.context("parsing json")
        }
        let mut results = vec![];
        match self {
            Request::Explanations(_) => {
                #[derive(serde::Deserialize)]
                struct JsonResponse {
                    target: String,
                    relations: EmptyStringOrVec<JsonRelation>,
                }
                let json = to_json::<JsonResponse>(response).await?;
                if let EmptyStringOrVec::Vec(relations) = json.relations {
                    for relation in relations {
                        let rhs = parse_handle(&relation.rhs)?;
                        let result = Relation {
                            lhs: parse_handle(&relation.lhs)?,
                            rhs: match parse_op(relation.op)? {
                                Operation::Eval => RelationRhs::Eval(rhs),
                                Operation::Apply => RelationRhs::Apply(rhs),
                            },
                        };
                        results.push(result)
                    }
                }
            }
            Request::Contents(h) => {
                #[derive(serde::Deserialize)]
                struct JsonResponse {
                    handles: EmptyStringOrVec<String>,
                }
                let json = to_json::<JsonResponse>(response).await?;
                let EmptyStringOrVec::Vec(entries) = json.handles else {
                    return Ok(vec![]);
                };
                let entries = entries
                    .into_iter()
                    .map(parse_handle)
                    .collect::<Result<Vec<_>>>()?;
                results = entries
                    .into_iter()
                    .enumerate()
                    .map(|(i, e)| Relation::new(h.clone(), RelationRhs::TreeEntry(e, i)))
                    .collect();
            }

            Request::Description(h) => {
                #[derive(serde::Deserialize)]
                struct JsonResponse {
                    description: String,
                }
                let json = to_json::<JsonResponse>(response).await?;
                results = vec![Relation::new(
                    h.clone(),
                    RelationRhs::Description(json.description),
                )];
            }
            Request::Relations(h, o) => {
                let json = to_json::<JsonRelation>(response).await?;
                let op = parse_op(json.op)?;
                ensure!(op == *o, "got different op back than requested");
                results = vec![Relation::new(
                    h.clone(),
                    match op {
                        Operation::Apply => RelationRhs::Apply,
                        Operation::Eval => RelationRhs::Eval,
                    }(parse_handle(json.rhs)?),
                )];
            }
        }
        Ok(results)
    }

    pub(crate) fn parse_send(request: String, ctx: HttpContext) {
        let count = ctx.counter.fetch_add(1, Ordering::SeqCst);
        let _ = ctx
            .tx
            .send(Ok(vec![(count, LogEntry::Request(request.clone()))]));
        let request = match Self::from_cli(&request) {
            Ok(v) => v,
            Err(e) => {
                let _ = ctx.tx.send(Err(e.context("parsing cli")));
                return;
            }
        };
        let task = async move {
            let result = ctx
                .client
                .get(format!("http://{}{}", ctx.url_base, request.to_url_path()))
                .send()
                .await;
            match result {
                Ok(ok) => {
                    let _ = ctx.tx.send(request.parse(ok).await.map(|v| {
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

    pub(crate) fn send(self, ctx: HttpContext) {
        Self::parse_send(self.to_cli(), ctx)
    }
}

impl HttpLog {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        HttpLog {
            tx,
            rx,
            log: vec![],
            command_input: String::new(),
        }
    }
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

fn parse_op(op: impl AsRef<str>) -> Result<Operation> {
    op.as_ref()
        .parse::<u8>()
        .context("parsing op")
        .and_then(|i| TryInto::<Operation>::try_into(i))
        .context("parsing op")
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
