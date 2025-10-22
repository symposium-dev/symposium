use std::collections::HashMap;
use std::pin::pin;

use futures::AsyncBufReadExt as _;
use futures::AsyncRead;
use futures::AsyncWrite;
use futures::AsyncWriteExt as _;
use futures::StreamExt;
use futures::channel::mpsc;
use futures::io::BufReader;
use uuid::Uuid;

use crate::jsonrpc::JsonRpcCx;
use crate::jsonrpc::JsonRpcHandler;
use crate::jsonrpc::JsonRpcRequestCx;
use crate::jsonrpc::OutgoingMessage;
use crate::jsonrpc::ReplyMessage;
use crate::util::internal_error;

use super::Handled;

/// The "reply actor" manages a queue of pending replies.
pub(super) async fn reply_actor(
    mut reply_rx: mpsc::UnboundedReceiver<ReplyMessage>,
) -> Result<(), jsonrpcmsg::Error> {
    // Map from the `id` to a oneshot sender where we should send the value.
    let mut map = HashMap::new();

    while let Some(message) = reply_rx.next().await {
        match message {
            ReplyMessage::Subscribe(id, message_tx) => {
                // total hack: id's don't implement Eq
                tracing::trace!(?id, "reply_actor: subscribing to response");
                let id = serde_json::to_value(&id).unwrap();
                map.insert(id, message_tx);
            }
            ReplyMessage::Dispatch(id, value) => {
                let id_debug = &id;
                let is_ok = value.is_ok();
                tracing::trace!(?id_debug, is_ok, "reply_actor: dispatching response");
                let id = serde_json::to_value(&id).unwrap();
                if let Some(message_tx) = map.remove(&id) {
                    // If the receiver is no longer interested in the reply,
                    // that's ok with us.
                    let result = message_tx.send(value);
                    if result.is_err() {
                        tracing::warn!(
                            ?id,
                            "reply_actor: failed to send response, receiver dropped"
                        );
                    } else {
                        tracing::trace!(
                            ?id,
                            "reply_actor: successfully dispatched response to receiver"
                        );
                    }
                } else {
                    tracing::warn!(
                        ?id,
                        "reply_actor: received response for unknown id, no subscriber found"
                    );
                }
            }
        }
    }
    Ok(())
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
/// - `Result<(), jsonrpcmsg::Error>`: an error if something unrecoverable occurred
pub(super) async fn incoming_actor(
    json_rpc_cx: &JsonRpcCx,
    incoming_bytes: impl AsyncRead,
    reply_tx: mpsc::UnboundedSender<ReplyMessage>,
    mut handler: impl JsonRpcHandler,
) -> Result<(), jsonrpcmsg::Error> {
    let incoming_bytes = pin!(incoming_bytes);
    let buffered_incoming_bytes = BufReader::new(incoming_bytes);
    let mut incoming_lines = buffered_incoming_bytes.lines();
    while let Some(line) = incoming_lines.next().await {
        let line = line.map_err(internal_error)?;
        tracing::trace!(message = %line, "Received JSON-RPC message");
        let message: Result<jsonrpcmsg::Message, _> = serde_json::from_str(&line);
        match message {
            Ok(msg) => match msg {
                jsonrpcmsg::Message::Request(request) => {
                    tracing::trace!(method = %request.method, id = ?request.id, "Handling request");
                    dispatch_request(json_rpc_cx, request, &mut handler).await?
                }
                jsonrpcmsg::Message::Response(response) => {
                    tracing::trace!(id = ?response.id, has_result = response.result.is_some(), has_error = response.error.is_some(), "Handling response");
                    if let Some(id) = response.id {
                        if let Some(value) = response.result {
                            reply_tx
                                .unbounded_send(ReplyMessage::Dispatch(id, Ok(value)))
                                .map_err(internal_error)?;
                        } else if let Some(error) = response.error {
                            reply_tx
                                .unbounded_send(ReplyMessage::Dispatch(id, Err(error)))
                                .map_err(internal_error)?;
                        }
                    }
                }
            },
            Err(_) => {
                json_rpc_cx.send_error_notification(jsonrpcmsg::Error::parse_error())?;
            }
        }
    }
    Ok(())
}

/// Dispatches a JSON-RPC request to the handler.
/// Report an error back to the server if it does not get handled.
async fn dispatch_request(
    json_rpc_cx: &JsonRpcCx,
    request: jsonrpcmsg::Request,
    handler: &mut impl JsonRpcHandler,
) -> Result<(), jsonrpcmsg::Error> {
    if let Some(id) = request.id {
        let request_cx = JsonRpcRequestCx::new(json_rpc_cx.clone(), id.clone());
        tracing::debug!(method = %request.method, ?id, "Dispatching request to handler");
        let handled = handler
            .handle_request(&request.method, &request.params, request_cx)
            .await?;

        match handled {
            Handled::Yes => {
                tracing::debug!(method = %request.method, ?id, "Handler reported: Handled::Yes");
            }
            Handled::No(request_cx) => {
                tracing::debug!(method = %request.method, ?id, "Handler reported: Handled::No, sending method_not_found");
                request_cx.respond_with_error(jsonrpcmsg::Error::method_not_found())?;
            }
        }
    } else {
        let handled = handler
            .handle_notification(&request.method, &request.params, json_rpc_cx)
            .await?;

        match handled {
            Handled::Yes => (),
            Handled::No(()) => {
                json_rpc_cx.send_error_notification(jsonrpcmsg::Error::method_not_found())?;
            }
        }
    }

    Ok(())
}

/// Actor processing outgoing messages and serializing them onto the transport.
///
/// # Parameters
///
/// * `outgoing_rx`: Receiver for outgoing messages.
/// * `reply_tx`: Sender for reply messages.
/// * `outgoing_bytes`: AsyncWrite for sending messages.
pub(super) async fn outgoing_actor(
    mut outgoing_rx: mpsc::UnboundedReceiver<OutgoingMessage>,
    reply_tx: mpsc::UnboundedSender<ReplyMessage>,
    outgoing_bytes: impl AsyncWrite,
) -> Result<(), jsonrpcmsg::Error> {
    let mut outgoing_bytes = pin!(outgoing_bytes);

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
                reply_tx
                    .unbounded_send(ReplyMessage::Subscribe(id.clone(), response_rx))
                    .map_err(internal_error)?;

                jsonrpcmsg::Message::Request(jsonrpcmsg::Request::new_v2(method, params, Some(id)))
            }
            OutgoingMessage::Notification { method, params } => {
                jsonrpcmsg::Message::Request(jsonrpcmsg::Request::new_v2(method, params, None))
            }
            OutgoingMessage::Response {
                id,
                response: Ok(value),
            } => {
                tracing::debug!(?id, "Sending success response");
                jsonrpcmsg::Message::Response(jsonrpcmsg::Response::success_v2(value, Some(id)))
            }
            OutgoingMessage::Response {
                id,
                response: Err(error),
            } => {
                tracing::warn!(?id, ?error, "Sending error response");
                jsonrpcmsg::Message::Response(jsonrpcmsg::Response::error_v2(error, Some(id)))
            }
            OutgoingMessage::Error { error } => {
                jsonrpcmsg::Message::Response(jsonrpcmsg::Response::error_v2(error, None))
            }
        };

        match serde_json::to_vec(&json_rpc_message) {
            Ok(mut bytes) => {
                if let Ok(msg_str) = std::str::from_utf8(&bytes) {
                    tracing::trace!(message = %msg_str, "Sending JSON-RPC message");
                }
                bytes.push('\n' as u8);
                outgoing_bytes
                    .write_all(&bytes)
                    .await
                    .map_err(internal_error)?;
            }

            Err(serialization_error) => {
                match json_rpc_message {
                    jsonrpcmsg::Message::Request(_request) => {
                        // If we failed to serialize a request,
                        // just ignore it.
                        //
                        // Q: (Maybe it'd be nice to "reply" with an error?)
                        tracing::error!(
                            ?serialization_error,
                            "Failed to serialize request, ignoring"
                        );
                    }
                    jsonrpcmsg::Message::Response(response) => {
                        // If we failed to serialize a *response*,
                        // send an error in response.
                        tracing::error!(?serialization_error, id = ?response.id, "Failed to serialize response, sending internal_error instead");
                        outgoing_bytes
                            .write_all(
                                &serde_json::to_vec(&jsonrpcmsg::Response::error(
                                    jsonrpcmsg::Error::internal_error(),
                                    response.id,
                                ))
                                .unwrap(),
                            )
                            .await
                            .map_err(internal_error)?;
                    }
                }
            }
        };
    }
    Ok(())
}
