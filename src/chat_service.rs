use crate::login_service::{ User, UserId, UserEvent };
use crate::locale::Locale;

use futures::{
    prelude::*,
    channel::mpsc::{UnboundedSender, UnboundedReceiver, unbounded}
};

use chrono::prelude::*;

use std::collections::HashMap;

pub struct ChatService {
    user_sender: UnboundedSender<UserEvent>,
    user_receiver: UnboundedReceiver<UserEvent>,
    users: HashMap<UserId, User>,
    locale: Locale,
}

impl ChatService {
    pub fn new(locale: Locale) -> Self {
        let (user_sender, user_receiver) = unbounded();
        ChatService { user_sender, user_receiver, locale, users: HashMap::new() }
    }

    pub fn make_user_handler(&self) -> UnboundedSender<UserEvent> {
        self.user_sender.clone()
    }

    pub async fn run(&mut self) {
        loop {
            match self.user_receiver.next().await {
                Some(UserEvent::NewUser(user)) => self.handle_new_user(user),
                Some(UserEvent::NewMessage(id, data)) => self.handle_new_message(id, data),
                Some(UserEvent::DropUser(id)) => self.handle_drop_user(id),
                None => panic!("ChatService user_receiver terminated"),
            }
        }
    }

    fn handle_new_user(&mut self, user: User) {
        let id = user.get_id();
        self.broadcast_except(id, format!("{} Connected: {}\n",
                                          Local::now().format("%H:%M"),
                                          user.get_login()));
        self.users.insert(id, user);
    }

    fn handle_new_message(&mut self, id: UserId, message: String) {
        if let Some(user) = self.users.get_mut(&id) {
            let message = format!("{} [{}] {}\n",
                                  Local::now().format("%H:%M"),
                                  user.get_login(),
                                  message);
            self.broadcast(message);
        }
    }

    fn handle_drop_user(&mut self, id: UserId) {
        if let Some(user) = self.users.remove(&id) {
            self.broadcast(format!("{} Disconnected: {}\n",
                                   Local::now().format("%H:%M"),
                                   user.get_login()));
        }
    }

    fn broadcast(&mut self, message: String) {
        for user in self.users.values_mut() {
            user.send(message.clone());
        }
    }

    fn broadcast_except(&mut self, id: UserId, message: String) {
        for user in self.users.values_mut() {
            if user.get_id() != id {
                user.send(message.clone());
            }
        }
    }
}
