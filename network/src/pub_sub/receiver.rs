use std::sync::Arc;
use std::time::Instant;

use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::detailed;
use crate::message::NetMessage;
use crate::metrics::NetMetrics;
use crate::pub_sub::connection::ConnectionWrapper;
use crate::pub_sub::connection::IncomingMessage;
use crate::pub_sub::is_safely_closed;
use crate::pub_sub::IncomingSender;
use crate::DeliveryPhase;

pub async fn receiver(
    metrics: Option<NetMetrics>,
    connection: Arc<ConnectionWrapper>,
    mut stop_rx: watch::Receiver<bool>,
    incoming_tx: IncomingSender,
) -> anyhow::Result<()> {
    let (transfer_result_tx, mut transfer_result_rx) = mpsc::channel(10);
    loop {
        tokio::select! {
            accept_result = connection.connection.accept_uni() => {
                match accept_result {
                    Ok(stream) => {
                        let metrics = metrics.clone();
                        let transfer_result_tx = transfer_result_tx.clone();
                        let connection = connection.clone();

                        // It is not critical task because it handles single income message
                        tokio::spawn(async move {
                            metrics.as_ref().inspect(|x|
                                x.start_delivery_phase(DeliveryPhase::IncomingTransfer, 1, "", connection.peer_send_mode())
                            );
                            tracing::trace!(
                                broadcast = connection.peer_send_mode().is_broadcast(),
                                peer = connection.peer_info(),
                                "Incoming transfer start",
                            );
                            let transfer_duration = Instant::now();
                            let transfer_result = transfer(stream).await;
                            metrics.as_ref().inspect(|x| {
                                x.finish_delivery_phase(
                                    DeliveryPhase::IncomingTransfer,
                                    1,
                                    "",
                                    connection.peer_send_mode(),
                                    transfer_duration.elapsed(),
                                );
                            });
                            transfer_result_tx.send(transfer_result).await
                        });
                    }
                    Err(err) => {
                        if !is_safely_closed(&err) {
                            tracing::error!(
                                peer = connection.peer_info(),
                                "Failed to accept incoming stream: {}",
                                detailed(&err)
                            );
                        }
                        // finish receiver loop because we have problem with this connection
                        break;
                    }
                }
            },
            transfer_result = transfer_result_rx.recv() => {
                // It is impossible to recv None, because we holds transfer_result_tx
                match transfer_result.unwrap() {
                    Ok(net_message) => {
                        tracing::debug!(
                            broadcast = connection.peer_send_mode().is_broadcast(),
                            msg_type = net_message.label,
                            msg_id = net_message.id,
                            peer = connection.peer_info(),
                            peer_id = connection.peer_id,
                            size = net_message.data.len(),
                            "Message delivery: incoming transfer finished",
                        );
                        metrics.as_ref().inspect(|x| {
                            x.start_delivery_phase(
                                DeliveryPhase::IncomingBuffer,
                                1,
                                &net_message.label,
                                connection.peer_send_mode(),
                            );
                        });
                        let incoming = IncomingMessage {
                            peer: connection.clone(),
                            message: net_message,
                            duration_after_transfer: Instant::now(),
                        };
                        // finish receiver loop if incoming consumer was detached
                        if incoming_tx.send(incoming).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::error!(
                            broadcast = connection.peer_send_mode().is_broadcast(),
                            peer = connection.peer_info(),
                            "Incoming transfer failed: {}",
                            detailed(&err)
                        );
                        // finish receiver loop because we have problem with this connection
                        break;
                    }
                }
            },
            _ = stop_rx.changed() => if *stop_rx.borrow() {
                break;
            }
        }
    }
    tracing::trace!(peer = connection.peer_info(), "Receiver loop finished");
    Ok(())
}

async fn transfer(mut stream: wtransport::RecvStream) -> anyhow::Result<NetMessage> {
    let mut data = Vec::with_capacity(1024);
    stream.read_to_end(&mut data).await?;
    Ok(bincode::deserialize::<NetMessage>(&data)?)
}
