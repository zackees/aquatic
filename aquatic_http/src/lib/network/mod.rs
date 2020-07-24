use std::time::{Duration, Instant};
use std::io::{ErrorKind, Cursor};
use std::sync::Arc;
use std::vec::Drain;

use hashbrown::HashMap;
use log::{info, debug, error};
use native_tls::TlsAcceptor;
use mio::{Events, Poll, Interest, Token};
use mio::net::TcpListener;

use aquatic_http_protocol::response::*;

use crate::common::*;
use crate::config::Config;

pub mod connection;
pub mod stream;
pub mod utils;

use connection::*;
use utils::*;


const CONNECTION_CLEAN_INTERVAL: usize = 2 ^ 22;


pub fn run_socket_worker(
    config: Config,
    socket_worker_index: usize,
    socket_worker_statuses: SocketWorkerStatuses,
    request_channel_sender: RequestChannelSender,
    response_channel_receiver: ResponseChannelReceiver,
    opt_tls_acceptor: Option<TlsAcceptor>,
){
    match create_listener(config.network.address, config.network.ipv6_only){
        Ok(listener) => {
            socket_worker_statuses.lock()[socket_worker_index] = Some(Ok(()));

            run_poll_loop(
                config,
                socket_worker_index,
                request_channel_sender,
                response_channel_receiver,
                listener,
                opt_tls_acceptor
            );
        },
        Err(err) => {
            socket_worker_statuses.lock()[socket_worker_index] = Some(
                Err(format!("Couldn't open socket: {:#}", err))
            );
        }
    }
}


pub fn run_poll_loop(
    config: Config,
    socket_worker_index: usize,
    request_channel_sender: RequestChannelSender,
    response_channel_receiver: ResponseChannelReceiver,
    listener: ::std::net::TcpListener,
    opt_tls_acceptor: Option<TlsAcceptor>,
){
    let poll_timeout = Duration::from_micros(
        config.network.poll_timeout_microseconds
    );

    let mut listener = TcpListener::from_std(listener);
    let mut poll = Poll::new().expect("create poll");
    let mut events = Events::with_capacity(config.network.poll_event_capacity);

    poll.registry()
        .register(&mut listener, Token(0), Interest::READABLE)
        .unwrap();

    let mut connections: ConnectionMap = HashMap::new();
    let opt_tls_acceptor = opt_tls_acceptor.map(Arc::new);

    let mut poll_token_counter = Token(0usize);
    let mut iter_counter = 0usize;

    let mut response_buffer = [0u8; 4096];
    let mut response_buffer = Cursor::new(&mut response_buffer[..]);
    let mut local_responses = Vec::new();

    loop {
        poll.poll(&mut events, Some(poll_timeout))
            .expect("failed polling");

        for event in events.iter(){
            let token = event.token();

            if token.0 == 0 {
                accept_new_streams(
                    &config,
                    &mut listener,
                    &mut poll,
                    &mut connections,
                    &mut poll_token_counter,
                    &opt_tls_acceptor,
                );
            } else {
                handle_connection_read_event(
                    &config,
                    socket_worker_index,
                    &request_channel_sender,
                    &mut local_responses,
                    &mut connections,
                    token,
                );
            }
        }

        if !(local_responses.is_empty() & (response_channel_receiver.is_empty())) {
            send_responses(
                &mut response_buffer,
                local_responses.drain(..),
                response_channel_receiver.try_iter(),
                &mut connections
            );
        }

        // Remove inactive connections, but not every iteration
        if iter_counter % CONNECTION_CLEAN_INTERVAL == 0 {
            remove_inactive_connections(&mut connections);
        }

        iter_counter = iter_counter.wrapping_add(1);
    }
}


fn accept_new_streams(
    config: &Config,
    listener: &mut TcpListener,
    poll: &mut Poll,
    connections: &mut ConnectionMap,
    poll_token_counter: &mut Token,
    opt_tls_acceptor: &Option<Arc<TlsAcceptor>>,
){
    let valid_until = ValidUntil::new(config.cleaning.max_connection_age);

    loop {
        match listener.accept(){
            Ok((mut stream, _)) => {
                poll_token_counter.0 = poll_token_counter.0.wrapping_add(1);

                if poll_token_counter.0 == 0 {
                    poll_token_counter.0 = 1;
                }

                let token = *poll_token_counter;

                // Remove connection if it exists (which is unlikely)
                connections.remove(&token);

                poll.registry()
                    .register(&mut stream, token, Interest::READABLE)
                    .unwrap();

                let connection = Connection::new(
                    opt_tls_acceptor,
                    valid_until,
                    stream
                );

                connections.insert(token, connection);
            },
            Err(err) => {
                if err.kind() == ErrorKind::WouldBlock {
                    break
                }

                info!("error while accepting streams: {}", err);
            }
        }
    }
}


/// On the stream given by poll_token, get TLS up and running if requested,
/// then read requests and pass on through channel.
pub fn handle_connection_read_event(
    config: &Config,
    socket_worker_index: usize,
    request_channel_sender: &RequestChannelSender,
    local_responses: &mut Vec<(ConnectionMeta, Response)>,
    connections: &mut ConnectionMap,
    poll_token: Token,
){
    let valid_until = ValidUntil::new(config.cleaning.max_connection_age);

    loop {
        // Get connection, updating valid_until
        let connection = if let Some(c) = connections.get_mut(&poll_token){
            c
        } else {
            // If there is no connection, there is no stream, so there
            // shouldn't be any (relevant) poll events. In other words, it's
            // safe to return here
            return
        };

        connection.valid_until = valid_until;

        if let Some(established) = connection.get_established(){
            match established.read_request(){
                Ok(request) => {
                    let meta = ConnectionMeta {
                        worker_index: socket_worker_index,
                        poll_token,
                        peer_addr: established.peer_addr
                    };

                    debug!("read request, sending to handler");

                    if let Err(err) = request_channel_sender
                        .send((meta, request))
                    {
                        error!(
                            "RequestChannelSender: couldn't send message: {:?}",
                            err
                        );
                    }

                    break
                },
                Err(RequestReadError::NeedMoreData) => {
                    info!("need more data");

                    // Stop reading data (defer to later events)
                    break;
                },
                Err(RequestReadError::Parse(err)) => {
                    info!("error reading request (invalid): {:#?}", err);

                    let meta = ConnectionMeta {
                        worker_index: socket_worker_index,
                        poll_token,
                        peer_addr: established.peer_addr
                    };

                    let response = FailureResponse {
                        failure_reason: "invalid request".to_string()
                    };

                    local_responses.push(
                        (meta, Response::Failure(response))
                    );

                    break;
                },
                Err(RequestReadError::StreamEnded) => {
                    ::log::debug!("stream ended");
            
                    connections.remove(&poll_token);

                    break
                },
                Err(RequestReadError::Io(err)) => {
                    ::log::info!("error reading request (io): {}", err);
            
                    connections.remove(&poll_token);
    
                    break; 
                },
            }
        } else if let Some(handshake_machine) = connections.remove(&poll_token)
            .and_then(Connection::get_in_progress)
        {
            match handshake_machine.establish_tls(){
                Ok(established) => {
                    let connection = Connection::from_established(
                        valid_until,
                        established
                    );

                    connections.insert(poll_token, connection);
                },
                Err(TlsHandshakeMachineError::WouldBlock(machine)) => {
                    let connection = Connection::from_in_progress(
                        valid_until,
                        machine
                    );

                    connections.insert(poll_token, connection);

                    // Break and wait for more data
                    break
                },
                Err(TlsHandshakeMachineError::Failure(err)) => {
                    info!("tls handshake error: {}", err);

                    // TLS negotiation failed
                    break
                }
            }
        }
    }
}


/// Read responses from channel, send to peers
pub fn send_responses(
    buffer: &mut Cursor<&mut [u8]>,
    local_responses: Drain<(ConnectionMeta, Response)>,
    channel_responses: crossbeam_channel::TryIter<(ConnectionMeta, Response)>,
    connections: &mut ConnectionMap,
){
    for (meta, response) in local_responses.chain(channel_responses){
        if let Some(established) = connections.get_mut(&meta.poll_token)
            .and_then(Connection::get_established)
        {
            if established.peer_addr != meta.peer_addr {
                info!("socket worker error: peer socket addrs didn't match");

                continue;
            }

            buffer.set_position(0);

            let bytes_written = response.write(buffer).unwrap();

            match established.send_response(&buffer.get_mut()[..bytes_written]){
                Ok(()) => {
                    debug!("sent response");
                },
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    debug!("send response: would block");
                },
                Err(err) => {
                    info!("error sending response: {}", err);

                    connections.remove(&meta.poll_token);
                },
            }
        }
    }
}


// Close and remove inactive connections
pub fn remove_inactive_connections(
    connections: &mut ConnectionMap,
){
    let now = Instant::now();

    connections.retain(|_, connection| {
        connection.valid_until.0 >= now
    });

    connections.shrink_to_fit();
}
