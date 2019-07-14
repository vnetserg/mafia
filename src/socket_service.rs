use crate::util::{monitor, Monitored, FlatlineFuture};

use futures::{
    prelude::*,
    future::Fuse,
    select,
    channel::mpsc::{UnboundedSender, UnboundedReceiver, unbounded},
    io::{ReadHalf, WriteHalf},
};

use runtime::net::{TcpListener, TcpStream};

use std::{
    io,
    sync::Arc,
    net::{IpAddr, SocketAddr},
    collections::HashMap,
};

pub type SocketId = SocketAddr;

#[derive(Clone)]
pub struct SocketProxy {
    id: SocketId,
    channel: UnboundedSender<SocketRequest>,
}

pub enum SocketEvent {
    NewSocket(SocketProxy),
    NewMessage(SocketId, Box<str>),
    ClosedSocket(SocketId),
}

pub struct SocketService {
    event_handler: UnboundedSender<SocketEvent>,
    address: IpAddr,
    port: u16,
    socket_writer: HashMap<SocketId, Monitored<WriteHalf<TcpStream>>>,
    request_receiver: UnboundedReceiver<SocketRequest>,
    request_sender: UnboundedSender<SocketRequest>,
    read_receiver: UnboundedReceiver<ReadResult>,
    read_sender: UnboundedSender<ReadResult>,
}

enum SocketRequest {
    SendMessage(SocketId, SocketMessage),
    CloseSocket(SocketId),
}

enum SocketMessage {
    Static(&'static str),
    Boxed(Box<str>),
    Arc(Arc<str>),
}

struct SocketReader {
    id: SocketId,
    reader: ReadHalf<TcpStream>,
    flatline: Fuse<FlatlineFuture>,
    sender: UnboundedSender<ReadResult>,
    keep_running: bool,
}

enum ReadResult {
    Ok(SocketId, Box<str>),
    IoError(SocketId, io::Error),
    Utf8Error(SocketId, std::str::Utf8Error),
    Closed(SocketId),
}

impl SocketService {
    pub fn new(event_handler: UnboundedSender<SocketEvent>, address: IpAddr, port: u16) -> Self {
        let (request_sender, request_receiver) = unbounded();
        let (read_sender, read_receiver) = unbounded();
        SocketService {
            event_handler,
            address,
            port,
            socket_writer: HashMap::new(),
            request_receiver,
            request_sender,
            read_receiver,
            read_sender,
        }
    }

    pub async fn run(&mut self) -> std::io::Result<()> {
        let mut listener = TcpListener::bind((self.address, self.port))?;
        println!("Listening on {}", listener.local_addr()?);

        let mut connections = listener.incoming();

        loop {
            select! {
                maybe_stream = connections.next().fuse() => {
                    self.handle_connection(maybe_stream
                                           .expect("SocketService connections stream terminated")?);
                },
                maybe_read = self.read_receiver.next().fuse() => {
                    if let Some(result) = maybe_read {
                        self.handle_read(result);
                    } else {
                        panic!("SocketService read stream terminated");
                    }
                },
                maybe_request = self.request_receiver.next().fuse() => {
                    self.handle_request(maybe_request
                                        .expect("SocketService request stream terminated")).await;
                },
            }
        }
    }

    fn handle_connection(&mut self, stream: TcpStream) {
        if let Ok(id) = stream.peer_addr() {
            eprintln!("New connection from {}", id);
            let proxy = SocketProxy{ id, channel: self.request_sender.clone() };
            let (reader, writer) = stream.split();
            let (monitored, flatline) = monitor(writer);
            self.socket_writer.insert(id, monitored);

            #[allow(unused)] {
                runtime::spawn(SocketReader::run(id, reader, flatline, self.read_sender.clone()));
            }

            self.event_handler.unbounded_send(SocketEvent::NewSocket(proxy))
                .expect("SocketService event_handler stream error");
        }
    }

    fn handle_read(&mut self, result: ReadResult) {
        match result {
            ReadResult::Ok(id, data) => {
                eprintln!("Received {} bytes from {}", data.len(), id);
                self.event_handler.unbounded_send(SocketEvent::NewMessage(id, data))
                    .expect("SocketService event_handler stream error");
            },
            ReadResult::Closed(id) => {
                eprintln!("Remote closed connection: {}", id);
                self.close_connection(id);
            },
            ReadResult::Utf8Error(id, _) => {
                eprintln!("Closing connection to {}: invalid utf-8", id);
                self.close_connection(id);
            },
            ReadResult::IoError(id, err) => {
                eprintln!("Closing connection to {}: write error {}", id, err);
                self.close_connection(id);
            },
        }
    }

    fn close_connection(&mut self, id: SocketId) {
        if let Some(mut writer) = self.socket_writer.remove(&id) {
            writer.close();
            self.event_handler.unbounded_send(SocketEvent::ClosedSocket(id))
                .expect("SocketService event_handler stream error");
        }
    }

    async fn handle_request(&mut self, request: SocketRequest) {
        match request {
            SocketRequest::SendMessage(id, message) => {
                if let Some(writer) = self.socket_writer.get_mut(&id) {
                    let data = match &message {
                        SocketMessage::Static(string) => string.as_bytes(),
                        SocketMessage::Boxed(string) => string.as_bytes(),
                        SocketMessage::Arc(string) => string.as_bytes(),
                    };
                    if let Err(err) = writer.write_all(&data).await {
                        eprintln!("Closing connection to {}: write error {}", id, err);
                        self.close_connection(id);
                    }
                }
            },
            SocketRequest::CloseSocket(id) => {
                if let Some(_) = self.socket_writer.get_mut(&id) {
                    eprintln!("Closing connection to {}", id);
                    self.close_connection(id);
                }
            },
        }
    }
}

impl SocketReader {
    const ERROR: &'static str = "SocketReader channel error";
    
    async fn run(
        id: SocketId,
        reader: ReadHalf<TcpStream>,
        flatline: FlatlineFuture,
        sender: UnboundedSender<ReadResult>
    ) {
        let flatline = flatline.fuse();
        let socket_reader = SocketReader{id, reader, flatline, sender, keep_running: true};
        socket_reader.read_forever().await
    }

    async fn read_forever(mut self) {
        let mut buffer: [u8; 1024] = [0; 1024];
        while self.keep_running {
            select! {
                result = self.reader.read(&mut buffer).fuse() => {
                    match result {
                        Ok(len) => self.handle_data(&buffer[..len]),
                        Err(err) => {
                            self.sender.unbounded_send(ReadResult::IoError(self.id, err))
                                .expect(Self::ERROR);
                            return;
                        }
                    }
                },
                _ = &mut self.flatline => return,
            }
        }
    }

    fn handle_data(&mut self, data: &[u8]) {
        if data.is_empty() {
            self.sender.unbounded_send(ReadResult::Closed(self.id))
                .expect(Self::ERROR);
            self.keep_running = false;
            return;
        }

        let mut start = 0;
        for i in 0..data.len() {
            if data[i] == b'\n' {
                match std::str::from_utf8(&data[start..i]) {
                    Ok(line) => {
                        self.sender.unbounded_send(ReadResult::Ok(self.id, line.trim().into()))
                            .expect(Self::ERROR);
                    },
                    Err(err) => {
                        self.sender.unbounded_send(ReadResult::Utf8Error(self.id, err))
                            .expect(Self::ERROR);
                        self.keep_running = false;
                        return;
                    }
                }
                start = i+1;
            }
        }
    }
}

impl SocketProxy {
    const ERROR: &'static str = "SocketProxy channel error";

    pub fn get_id(&self) -> SocketId {
        self.id
    }

    pub fn send(&self, message: String) {
        self.send_boxed(message.into_boxed_str());
    }

    pub fn send_boxed(&self, message: Box<str>) {
        self.channel.unbounded_send(SocketRequest::SendMessage(self.id,
                                                               SocketMessage::Boxed(message)))
            .expect(Self::ERROR);
    }

    pub fn send_arc(&self, message: Arc<str>) {
        self.channel.unbounded_send(SocketRequest::SendMessage(self.id,
                                                               SocketMessage::Arc(message)))
            .expect(Self::ERROR);
    }

    pub fn send_static(&self, message: &'static str) {
        self.channel.unbounded_send(SocketRequest::SendMessage(self.id,
                                                               SocketMessage::Static(message)))
            .expect(Self::ERROR);
    }

    pub fn close(&self) {
        self.channel.unbounded_send(SocketRequest::CloseSocket(self.id)).expect(Self::ERROR);
    }
}
