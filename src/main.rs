#![feature(async_await)]
#![feature(async_closure)]
#![recursion_limit="128"]

mod chat_service;
mod login_service;
mod socket_service;
mod locale;
mod util;

use socket_service::SocketService;
use login_service::LoginService;
use chat_service::ChatService;
use locale::Locale;

use futures::prelude::*;
use futures::select;

use std::net::IpAddr;

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
    let mut chat_service = ChatService::new(args.locale);
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

    select! {
        res = socket_task => {
            if res.is_err() {
                eprintln!("Socket service failed");
                return res;
            }
            eprintln!("Socket task exited");
        },
        _ = login_task => {
            eprintln!("Login task exited");
        },
        _ = chat_task => {
            eprintln!("Chat task exited");
        }
    }

    Ok(())

}
