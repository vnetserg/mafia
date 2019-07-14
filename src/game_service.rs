use crate::chat_service::{GameEvent, Player, PlayerId, MuteLevel};
use crate::locale::Locale;

use futures::{
    prelude::*,
    channel::mpsc::{UnboundedSender, UnboundedReceiver, unbounded}
};

pub struct GameService {
    event_sender: UnboundedSender<GameEvent>,
    event_receiver: UnboundedReceiver<GameEvent>,
    locale: Locale,
}

impl GameService {
    pub fn new(locale: Locale) -> Self {
        let (event_sender, event_receiver) = unbounded();
        GameService {
            event_sender,
            event_receiver,
            locale,
        }
    }

    pub fn make_event_handler(&self) -> UnboundedSender<GameEvent> {
        self.event_sender.clone()
    }

    pub async fn run(&mut self) {
        loop {
            match self.event_receiver.next().await {
                Some(GameEvent::Connected(player)) => player.mute(MuteLevel::AllowAll),
                _ => (),
            }
        }
    }
}
