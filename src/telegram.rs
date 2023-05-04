use std::time::Duration;

use serde::{Deserialize, Serialize};
use teloxide::{
    payloads::{SendChatAction, SendMessage, SendVoice},
    types::{ChatAction, ChatId, Message, ParseMode},
};
use tokio::{
    sync::broadcast::{self, Sender},
    time::interval,
};

use crate::util::new_reqwest_client;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Telegram {
    pub token: String,
}

impl Telegram {
    pub fn new(token: impl ToString) -> Self {
        Self {
            token: token.to_string(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("TELEGRAM_TOKEN").unwrap())
    }

    pub async fn send_message(&self, message: &SendMessage) {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        println!("{}", serde_json::to_string(message).unwrap());
        let result = new_reqwest_client()
            .post(&url)
            .json(message)
            .send()
            .await
            .unwrap();
        println!("{:?}", result);
    }

    pub async fn send_voice(&self, message: &SendVoice) {
        let url = format!("https://api.telegram.org/bot{}/sendVoice", self.token);
        new_reqwest_client()
            .post(&url)
            .form(message)
            .send()
            .await
            .unwrap();
    }

    pub fn start_sending_typing_status(&self, chat_id: ChatId) -> Sender<()> {
        let (stop_typing_action_tx, mut stop_typing_action_rx) = broadcast::channel(1);
        let token = self.token.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = stop_typing_action_rx.recv() => {
                        break;
                    }
                    _ = interval.tick() => {
                        let message = SendChatAction::new(chat_id, ChatAction::Typing);
                        new_reqwest_client()
                            .post(format!("https://api.telegram.org/bot{token}/sendChatAction"))
                            .json(&message)
                            .send()
                            .await
                            .unwrap();
                    }
                }
            }
        });
        stop_typing_action_tx
    }
}

pub fn escape(text: &str) -> String {
    text.replace("\"", "\\\"")
        .replace("{", "\\{")
        .replace("}", "\\}")
        .replace("_", "\\_")
        .replace(".", "\\.")
}

pub fn simple_respond_message(to_message: &Message, text: &str) -> SendMessage {
    let mut result = SendMessage::new(to_message.chat.id, escape(text));
    result.reply_to_message_id = Some(to_message.id);
    result.parse_mode = Some(ParseMode::MarkdownV2);
    result
}
