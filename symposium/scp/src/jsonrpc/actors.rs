use std::collections::HashMap;
use std::pin::Pin;

use futures::AsyncBufReadExt as _;
use futures::AsyncRead;
use futures::AsyncWrite;
use futures::AsyncWriteExt as _;
use futures::Stream;
use futures::StreamExt;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future::Either;
use futures::io::BufReader;
use uuid::Uuid;

use crate::jsonrpc::JsonRpcCx;
use crate::jsonrpc::JsonRpcHandler;
use crate::jsonrpc::JsonRpcRequestCx;
use crate::jsonrpc::OutgoingMessage;
use crate::jsonrpc::ReplyMessage;

/// The "reply actor" manages a queue of pending replies.
pub(super) async fn reply_actor(
    mut reply_rx: mpsc::UnboundedReceiver<ReplyMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Map from the `id` to a oneshot sender where we should send the value.
    let mut map = HashMap::new();

    while let Some(message) = reply_rx.next().await {
        match message {
            ReplyMessage::Subscribe(id, message_tx) => {
                // total hack: id's don't implement Eq
                let id = serde_json::to_value(&id).unwrap();
                map.insert(id, message_tx);
            }
            ReplyMessage::Dispatch(id, value) => {
                let id = serde_json::to_value(&id).unwrap();
                if let Some(message_tx) = map.remove(&id) {
                    // If the receiver is no longer interested in the reply,
                    // that's ok with us.
                    let _: Result<_, _> = message_tx.send(value);
                }
            }
        }
    }
    Ok(())
}

/// Read a single line from the input.
/// Return None if the input is closed or cancelled.
async fn read_next_line(
    incoming_lines: &mut (impl Stream<Item = Result<String, std::io::Error>> + Unpin),
    cancellation_tx: &mut oneshot::Sender<()>,
) -> Option<Result<String, Box<dyn std::error::Error>>> {
    let race_result =
        futures::future::select(incoming_lines.next(), cancellation_tx.cancellation()).await;

    match race_result {
        Either::Left((Some(Ok(next_line)), _)) => Some(Ok(next_line)),
        Either::Left((Some(Err(err)), _)) => Some(Err(err.into())),
        Either::Left((None, _)) | Either::Right(_) => None,
    }
}

/// Parsing incoming messages from `incoming_bytes`.
/// Each message will be dispatched to the appropriate layer.
///
/// # Parameters
/// - `json_rpc_cx`: The JSON-RPC context.
/// - `incoming_bytes`: The incoming bytes.
/// - `reply_tx`: The reply sender.
/// - `mut cancellation_tx`: cancellation signal; when the rx side of this channel is dropped, the actor will terminate
/// - `layers`: The layers.
///
/// # Returns
/// - `Result<(), Box<dyn std::error::Error>>`: an error if something unrecoverable occurred
pub(super) async fn incoming_actor(
    json_rpc_cx: &JsonRpcCx,
    incoming_bytes: Pin<Box<dyn AsyncRead>>,
    reply_tx: mpsc::UnboundedSender<ReplyMessage>,
    mut cancellation_tx: oneshot::Sender<()>,
    layers: Vec<Box<dyn JsonRpcHandler>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let buffered_incoming_bytes = BufReader::new(incoming_bytes);
    let mut incoming_lines = buffered_incoming_bytes.lines();
    while let Some(line) = read_next_line(&mut incoming_lines, &mut cancellation_tx).await {
        let line = line?;
        let message: Result<jsonrpcmsg::Message, _> = serde_json::from_str(&line);
        match message {
            Ok(msg) => match msg {
                jsonrpcmsg::Message::Request(request) => {
                    dispatch_request(json_rpc_cx, request, &layers).map_err(|err| err.message)?
                }
                jsonrpcmsg::Message::Response(response) => {
                    if let Some(id) = response.id {
                        if let Some(value) = response.result {
                            reply_tx.unbounded_send(ReplyMessage::Dispatch(id, Ok(value)))?;
                        } else if let Some(error) = response.error {
                            reply_tx.unbounded_send(ReplyMessage::Dispatch(id, Err(error)))?;
                        }
                    }
                }
            },
            Err(_) => {
                json_rpc_cx
                    .send_error_notification(jsonrpcmsg::Error::parse_error())
                    .map_err(|err| format!("failed to send error: {}", err.message))?;
            }
        }
    }
    Ok(())
}

/// Dispatches a JSON-RPC request to the first layer to claim it.
/// If no layer claims it, responds with an error.
fn dispatch_request(
    json_rpc_cx: &JsonRpcCx,
    request: jsonrpcmsg::Request,
    layers: &[Box<dyn JsonRpcHandler>],
) -> Result<(), jsonrpcmsg::Error> {
    if let Some(id) = request.id {
        // Create the respond object with the request id
        let mut request_cx = JsonRpcRequestCx::new(json_rpc_cx.clone(), id);

        // Search for a layer that can handle this kind of request
        for layer in layers {
            match layer.claim_request(&request.method, &request.params, request_cx) {
                Ok(()) => return Ok(()),
                Err(t) => request_cx = t,
            }
        }

        // If none found, send an error response
        request_cx.respond_with_error(jsonrpcmsg::Error::method_not_found())
    } else {
        // Search for a layer that can handle this kind of notification
        for layer in layers {
            match layer.claim_notification(&request.method, &request.params) {
                Ok(()) => return Ok(()),
                Err(()) => (),
            }
        }

        // If none found, ignore.
        Ok(())
    }
}

/// Actor processing outgoing messages and serializing them onto the transport.
///
/// # Parameters
///
/// * `outgoing_rx`: Receiver for outgoing messages.
/// * `reply_tx`: Sender for reply messages.
/// * `outgoing_bytes`: AsyncWrite for sending messages.
/// * `_incoming_cancel_rx`: Dropped when we return, which signals the incoming message actor to stop.
pub(super) async fn outgoing_actor(
    mut outgoing_rx: mpsc::UnboundedReceiver<OutgoingMessage>,
    reply_tx: mpsc::UnboundedSender<ReplyMessage>,
    mut outgoing_bytes: Pin<Box<dyn AsyncWrite>>,
    _incoming_cancel_rx: oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    while let Some(message) = outgoing_rx.next().await {
        // Create the message to be sent over the transport
        let json_rpc_message = match message {
            OutgoingMessage::Request {
                method,
                params,
                response_tx: response_rx,
            } => {
                // Generate a fresh UUID to use for the request id
                let uuid = Uuid::new_v4();
                let id = jsonrpcmsg::Id::String(uuid.to_string());

                // Record where the reply should be sent once it arrives.
                reply_tx.unbounded_send(ReplyMessage::Subscribe(id.clone(), response_rx))?;

                jsonrpcmsg::Message::Request(jsonrpcmsg::Request::new_v2(method, params, Some(id)))
            }
            OutgoingMessage::Notification { method, params } => {
                jsonrpcmsg::Message::Request(jsonrpcmsg::Request::new_v2(method, params, None))
            }
            OutgoingMessage::Response {
                id,
                response: Ok(value),
            } => jsonrpcmsg::Message::Response(jsonrpcmsg::Response::success_v2(value, Some(id))),
            OutgoingMessage::Response {
                id,
                response: Err(error),
            } => jsonrpcmsg::Message::Response(jsonrpcmsg::Response::error_v2(error, Some(id))),
            OutgoingMessage::Error { error } => {
                jsonrpcmsg::Message::Response(jsonrpcmsg::Response::error_v2(error, None))
            }
        };

        match serde_json::to_vec(&json_rpc_message) {
            Ok(bytes) => {
                outgoing_bytes.write_all(&bytes).await?;
            }

            Err(_) => {
                match json_rpc_message {
                    jsonrpcmsg::Message::Request(_request) => {
                        // If we failed to serialize a request,
                        // just ignore it.
                        //
                        // Q: (Maybe it'd be nice to "reply" with an error?)
                    }
                    jsonrpcmsg::Message::Response(response) => {
                        // If we failed to serialize a *response*,
                        // send an error in response.
                        outgoing_bytes
                            .write_all(
                                &serde_json::to_vec(&jsonrpcmsg::Response::error(
                                    jsonrpcmsg::Error::internal_error(),
                                    response.id,
                                ))
                                .unwrap(),
                            )
                            .await?;
                    }
                }
            }
        };
    }
    Ok(())
}
