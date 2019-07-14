#![feature(async_await)]
#![feature(async_closure)]
#![feature(slice_partition_dedup)]
#![recursion_limit="128"]

mod game_service;
mod chat_service;
mod login_service;
mod socket_service;
mod locale;
mod util;

use game_service::GameService;
use chat_service::ChatService;
use login_service::LoginService;
use socket_service::SocketService;
use locale::Locale;

use futures::{
    select,
    prelude::*,
    channel::mpsc::unbounded,
};

use std::{
    net::IpAddr,
    process::exit,
};

struct Args {
    address: IpAddr,
    port: u16,
    locale: Locale,
}

impl Args {
    fn parse() -> Self {
        Args {
            address: [127, 0, 0, 1].into(),
            port: 8080,
            locale: Locale::En,
        }
    }
}

#[runtime::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let mut game_service = GameService::new(args.locale);
    let mut chat_service = ChatService::new(game_service.make_event_handler(), args.locale);
    let mut login_service = LoginService::new(chat_service.make_user_handler(), args.locale);
    let mut socket_service = SocketService::new(login_service.make_socket_handler(),
                                                args.address, args.port);

    let mut socket_task = runtime::spawn(async move {
        socket_service.run().await
    }).fuse();
    let mut login_task = runtime::spawn(async move {
        login_service.run().await
    }).fuse();
    let mut chat_task = runtime::spawn(async move {
        chat_service.run().await
    }).fuse();
    let mut game_task = runtime::spawn(async move {
        game_service.run().await
    }).fuse();

    let (ctrlc_sender, mut ctrlc_receiver) = unbounded();
    ctrlc::set_handler(move || {
        ctrlc_sender.unbounded_send(()).expect("Error sending Ctrl-C event");
    }).expect("Error setting Ctrl-C handler");

    select! {
        res = socket_task => {
            if let Err(err) = res {
                eprintln!("Socket service failed: {}.", err);
            } else {
                eprintln!("Socket service exited unexpectedly.");
            }
            exit(1)
        },
        _ = login_task => {
            eprintln!("Login service exited unexpectedly.");
            exit(1);
        },
        _ = chat_task => {
            eprintln!("Chat service exited unexpectedly.");
            exit(1);
        },
        _ = game_task => {
            eprintln!("Game service exited unexpectedly.");
            exit(1);
        },
        _ = ctrlc_receiver.next().fuse() => {
            eprintln!("User-requested shutdown.");
            exit(0);
        },
    }
}
