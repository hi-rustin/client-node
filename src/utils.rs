use std::ops::Bound;
use std::{sync::Arc, u32};

use neon::prelude::*;
use neon::{
    context::{Context, TaskContext},
    prelude::Handle,
    result::JsResultExt,
    types::{JsArray, JsString, JsValue},
};
use tikv_client::{Key, KvPair};

use tikv_client::TimestampExt;

use crate::{RawClient, Transaction, TransactionClient};
use lazy_static::lazy_static;
use tokio::{runtime::Runtime, sync::Mutex};

lazy_static! {
    pub(crate) static ref RUNTIME: Runtime = Runtime::new().unwrap();
}

pub fn bytes_to_js_string<'a>(cx: &mut TaskContext<'a>, bytes: Vec<u8>) -> Handle<'a, JsValue> {
    let content = std::str::from_utf8(&bytes).unwrap().to_owned();
    cx.string(content).upcast()
}

// pub fn bytes_to_js_string<'a>(cx: &'a mut TaskContext, bytes: Vec<u8>) -> Handle<'a, JsValue> {
//     let content = std::str::from_utf8(&bytes).unwrap().to_owned();
//     cx.string(content).upcast()
// }

pub enum CommonTypes {
    Unit(()),
    Keys(Vec<Key>),
    KvPairs(Vec<KvPair>),
    RawClient(tikv_client::RawClient),
    TransactionClient(tikv_client::TransactionClient),
    Transaction(tikv_client::Transaction),
    Timestamp(Option<tikv_client::Timestamp>),
}

impl From<()> for CommonTypes {
    fn from(_: ()) -> Self {
        CommonTypes::Unit(())
    }
}

impl From<Vec<Key>> for CommonTypes {
    fn from(item: Vec<Key>) -> Self {
        CommonTypes::Keys(item)
    }
}

impl From<Vec<KvPair>> for CommonTypes {
    fn from(item: Vec<KvPair>) -> Self {
        CommonTypes::KvPairs(item)
    }
}
impl From<tikv_client::RawClient> for CommonTypes {
    fn from(item: tikv_client::RawClient) -> Self {
        CommonTypes::RawClient(item)
    }
}
impl From<tikv_client::TransactionClient> for CommonTypes {
    fn from(item: tikv_client::TransactionClient) -> Self {
        CommonTypes::TransactionClient(item)
    }
}
impl From<tikv_client::Transaction> for CommonTypes {
    fn from(item: tikv_client::Transaction) -> Self {
        CommonTypes::Transaction(item)
    }
}
impl From<Option<tikv_client::Timestamp>> for CommonTypes {
    fn from(item: Option<tikv_client::Timestamp>) -> Self {
        CommonTypes::Timestamp(item)
    }
}

pub fn result_to_js_array<'a>(
    cx: &mut TaskContext<'a>,
    result: Result<CommonTypes, tikv_client::Error>,
) -> Vec<Handle<'a, JsValue>> {
    match result {
        Ok(values) => vec![
            cx.null().upcast(),
            match values {
                CommonTypes::Unit(_) => cx.undefined().upcast(),
                CommonTypes::Keys(keys) => rust_keys_to_js_array(cx, keys).upcast(),
                CommonTypes::KvPairs(pairs) => rust_pairs_to_js_array(cx, pairs).upcast(),
                CommonTypes::RawClient(client) => cx
                    .boxed(RawClient {
                        inner: Arc::new(client),
                    })
                    .upcast(),
                CommonTypes::TransactionClient(client) => cx
                    .boxed(TransactionClient {
                        inner: Arc::new(client),
                    })
                    .upcast(),
                CommonTypes::Transaction(client) => cx
                    .boxed(Transaction {
                        inner: Arc::new(Mutex::new(client)),
                    })
                    .upcast(),
                CommonTypes::Timestamp(timestamp) => match timestamp {
                    None => cx.undefined().upcast(),
                    Some(t) => cx.number(t.version() as f64).upcast(),
                },
            },
        ],
        Err(err) => vec![
            cx.error(err.to_string()).unwrap().upcast(),
            cx.undefined().upcast(),
        ],
    }
}

pub fn rust_pairs_to_js_array<'a>(
    cx: &mut TaskContext<'a>,
    values: Vec<KvPair>,
) -> Handle<'a, JsArray> {
    let js_array = JsArray::new(cx, values.len() as u32);
    for (i, obj) in values.iter().enumerate() {
        let pair = JsArray::new(cx, 2);
        let v1 = cx.string(
            std::str::from_utf8(&Vec::from(obj.0.clone()))
                .unwrap()
                .to_owned(),
        );
        let v2 = cx.string(std::str::from_utf8(&obj.1).unwrap().to_owned());
        pair.set(cx, 0, v1).unwrap();
        pair.set(cx, 1, v2).unwrap();
        js_array.set(cx, i as u32, pair).unwrap();
    }
    js_array
}

pub fn rust_keys_to_js_array<'a>(cx: &mut TaskContext<'a>, keys: Vec<Key>) -> Handle<'a, JsArray> {
    let js_array = JsArray::new(cx, keys.len() as u32);
    for (i, obj) in keys.iter().enumerate() {
        let v1 = cx.string(
            std::str::from_utf8(&Vec::from(obj.clone()))
                .unwrap()
                .to_owned(),
        );
        js_array.set(cx, i as u32, v1).unwrap();
    }
    js_array
}

pub fn js_array_to_rust_keys<'a>(
    cx: &mut FunctionContext<'a>,
    array: Handle<JsArray>,
) -> impl IntoIterator<Item = impl Into<Key>> {
    let array = array.to_vec(cx).unwrap(); // TODO: remove unwrap here
    array
        .into_iter()
        .map(|k| {
            k.downcast::<JsString, _>(cx)
                .or_throw(cx)
                .unwrap()
                .value(cx)
        })
        .collect::<Vec<String>>()
}

pub fn js_array_to_rust_pairs<'a>(
    cx: &mut FunctionContext<'a>,
    array: Handle<JsArray>,
) -> impl IntoIterator<Item = impl Into<KvPair>> {
    let array = array.to_vec(cx).unwrap(); // TODO: remove unwrap here
    let mut pairs = vec![];
    for k in array.into_iter() {
        let pair_result = k.downcast::<JsArray, _>(cx).or_throw(cx);
        match pair_result {
            Ok(pair) => {
                let args: Vec<String> = vec![0_u32, 1_u32]
                    .into_iter()
                    .map(|i| {
                        pair.get(cx, i as u32)
                            .unwrap()
                            .downcast::<JsString, _>(cx)
                            .or_throw(cx)
                            .unwrap() // TODO: remove unwrap here
                            .value(cx)
                    })
                    .collect();
                pairs.push(KvPair::new(
                    args.get(0).unwrap().to_owned(),
                    args.get(1).unwrap().to_owned(),
                ));
            }
            Err(err) => println!("{}", err.to_string()),
        }
    }
    pairs
}

pub fn to_bound_range(
    start: Option<Vec<u8>>,
    end: Option<Vec<u8>>,
    include_start: bool,
    include_end: bool,
) -> tikv_client::BoundRange {
    let start_bound = if let Some(start) = start {
        if include_start {
            Bound::Included(start)
        } else {
            Bound::Excluded(start)
        }
    } else {
        Bound::Unbounded
    };
    let end_bound = if let Some(end) = end {
        if include_end {
            Bound::Included(end)
        } else {
            Bound::Excluded(end)
        }
    } else {
        Bound::Unbounded
    };
    tikv_client::BoundRange::from((start_bound, end_bound))
}

pub fn send_result<T: Into<CommonTypes>>(
    queue: EventQueue,
    callback: Root<JsFunction>,
    result: Result<T, tikv_client::Error>,
) -> Result<(), neon::result::Throw> {
    let result = result.map(|values| values.into());
    queue.send(move |mut cx| {
        let callback = callback.into_inner(&mut cx);
        let this = cx.undefined();
        let args: Vec<Handle<JsValue>> = result_to_js_array(&mut cx, result);
        callback.call(&mut cx, this, args)?;
        Ok(())
    });
    Ok(())
}
